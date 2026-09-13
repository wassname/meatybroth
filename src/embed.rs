//! Builds local or Bedrock embeddings for admitted notes in the reader's SQLite file.
//!
//! Each vector space has model, revision, tokenizer, and content-hash provenance. Request rows are
//! the durable cost ledger; chunk and post vectors belong to SDK events. -- Pi/gpt-5.6-sol

use crate::{queries::WINDOW, Error};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Mutex,
};

pub const TITAN_MODEL: &str = "amazon.titan-embed-text-v2:0";
pub const TITAN_DIMENSIONS: usize = 512;
pub const MINILM_MODEL: &str = "sentence-transformers/all-MiniLM-L6-v2";
pub const MINILM_DIMENSIONS: usize = 384;
#[cfg(test)]
const TITAN_MAX_CHUNK_BYTES: usize = 8_000;
#[cfg(test)]
const TITAN_PRICE_NUSD_PER_TOKEN: i64 = 20;
const MINILM_MAX_SEQUENCE_TOKENS: usize = 256;
const MINILM_CONTENT_TOKENS: usize = MINILM_MAX_SEQUENCE_TOKENS - 2;

/// Hard limits checked against reserved and completed provider cost.
#[derive(Clone, Copy)]
pub struct Budget {
    pub total_nusd: i64,
    pub monthly_nusd: i64,
}

/// Content-addressed identity for one non-interchangeable embedding space.
#[derive(Clone, Debug)]
pub struct Space {
    pub id: String,
    pub backend: String,
    pub model: String,
    pub dimensions: usize,
    pub normalize: bool,
    pub revision: String,
    pub model_sha256: String,
    pub tokenizer_sha256: String,
    pub price_nusd_per_token: i64,
}

impl Space {
    fn content_addressed(mut self) -> Self {
        let provenance = format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}",
            self.backend,
            self.model,
            self.dimensions,
            self.normalize,
            self.revision,
            self.model_sha256,
            self.tokenizer_sha256,
        );
        self.id = hex_sha256(provenance.as_bytes());
        self
    }

    #[cfg(test)]
    pub fn titan_v2() -> Self {
        Self {
            id: String::new(),
            backend: "bedrock".into(),
            model: TITAN_MODEL.into(),
            dimensions: TITAN_DIMENSIONS,
            normalize: true,
            revision: "provider-managed".into(),
            model_sha256: "provider-managed".into(),
            tokenizer_sha256: "provider-managed".into(),
            price_nusd_per_token: TITAN_PRICE_NUSD_PER_TOKEN,
        }
        .content_addressed()
    }
}

/// Provider output and its billed or observed input-token count.
pub struct Output {
    pub vector: Vec<f32>,
    pub input_tokens: i64,
}

/// Model boundary used by local MiniLM, Bedrock, and deterministic tests.
pub trait Transport: Send + Sync {
    fn space(&self) -> &Space;
    fn split(&self, text: &str) -> Result<Vec<String>, Error>;
    fn embed<'a>(
        &'a self,
        text: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Output, Error>> + Send + 'a>>;
}

/// Native ONNX MiniLM inference with tokenizer-bound chunking.
pub struct MiniLm {
    model: Mutex<TextEmbedding>,
    space: Space,
}

impl MiniLm {
    /// Downloads or opens the pinned Hugging Face snapshot and records both content hashes.
    pub fn open(cache_dir: &Path) -> Result<Self, Error> {
        let model = TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_cache_dir(cache_dir.to_path_buf())
                .with_max_length(MINILM_MAX_SEQUENCE_TOKENS)
                .with_show_download_progress(true),
        )?;
        let (revision, model_sha256, tokenizer_sha256) = model_provenance(cache_dir)?;
        Ok(Self {
            model: Mutex::new(model),
            space: Space {
                id: String::new(),
                backend: "minilm".into(),
                model: MINILM_MODEL.into(),
                dimensions: MINILM_DIMENSIONS,
                normalize: true,
                revision,
                model_sha256,
                tokenizer_sha256,
                price_nusd_per_token: 0,
            }
            .content_addressed(),
        })
    }

    /// Returns the content-addressed vector-space provenance.
    pub fn vector_space(&self) -> &Space {
        &self.space
    }

    /// Embeds one tokenizer-bounded text without an async runtime boundary.
    pub fn embed_text(&self, text: &str) -> Result<Output, Error> {
        let mut model = self.model.lock().unwrap();
        let input_tokens = i64::try_from(model.tokenizer.encode(text, true)?.len())?;
        let mut vectors = model.embed(vec![text], None)?;
        if vectors.len() != 1 {
            return Err("MiniLM returned an unexpected batch size".into());
        }
        Ok(Output {
            vector: vectors.remove(0),
            input_tokens,
        })
    }
}

impl Transport for MiniLm {
    fn space(&self) -> &Space {
        &self.space
    }

    fn split(&self, text: &str) -> Result<Vec<String>, Error> {
        assert!(!text.trim().is_empty());
        let model = self.model.lock().unwrap();
        let mut tokenizer = model.tokenizer.clone();
        tokenizer.with_truncation(None)?;
        let encoding = tokenizer.encode(text, false)?;
        let offsets: Vec<_> = encoding
            .get_offsets()
            .iter()
            .copied()
            .filter(|(start, end)| end > start)
            .collect();
        if offsets.len() <= MINILM_CONTENT_TOKENS {
            return Ok(vec![text.to_owned()]);
        }
        let mut chunks = Vec::new();
        let mut start = 0;
        for tokens in offsets.chunks(MINILM_CONTENT_TOKENS) {
            let end = if chunks.len() + 1 == offsets.len().div_ceil(MINILM_CONTENT_TOKENS) {
                text.len()
            } else {
                tokens.last().unwrap().1
            };
            if end <= start || !text.is_char_boundary(end) {
                return Err("MiniLM tokenizer returned an invalid byte boundary".into());
            }
            chunks.push(text[start..end].to_owned());
            start = end;
        }
        if chunks.concat() != text {
            return Err("MiniLM chunking did not preserve the full input".into());
        }
        for chunk in &chunks {
            let encoded = tokenizer.encode(chunk.as_str(), false)?;
            if encoded.len() > MINILM_CONTENT_TOKENS {
                return Err("MiniLM chunk exceeded the configured token window".into());
            }
        }
        Ok(chunks)
    }

    fn embed<'a>(
        &'a self,
        text: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Output, Error>> + Send + 'a>> {
        Box::pin(async move { self.embed_text(text) })
    }
}

fn db(path: &Path) -> Result<Connection, Error> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    Ok(conn)
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sha256_file(path: &Path) -> Result<String, Error> {
    Ok(hex_sha256(&std::fs::read(path)?))
}

fn find_refs_main(path: &Path, found: &mut Vec<PathBuf>) -> Result<(), Error> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        if child.is_dir() {
            find_refs_main(&child, found)?;
        } else if child.ends_with("refs/main")
            && child.ancestors().any(|part| {
                part.file_name()
                    .is_some_and(|name| name == "models--Qdrant--all-MiniLM-L6-v2-onnx")
            })
        {
            found.push(child);
        }
    }
    Ok(())
}

fn model_provenance(cache_dir: &Path) -> Result<(String, String, String), Error> {
    let mut refs = Vec::new();
    find_refs_main(cache_dir, &mut refs)?;
    if refs.len() != 1 {
        return Err(format!("Expected one MiniLM refs/main file, found {}", refs.len()).into());
    }
    let revision = std::fs::read_to_string(&refs[0])?.trim().to_owned();
    let repository = refs[0].parent().unwrap().parent().unwrap();
    let snapshot = repository.join("snapshots").join(&revision);
    let model_sha256 = sha256_file(&snapshot.join("model.onnx"))?;
    let tokenizer_sha256 = sha256_file(&snapshot.join("tokenizer.json"))?;
    Ok((revision, model_sha256, tokenizer_sha256))
}

/// Splits UTF-8 text without changing its bytes or exceeding Titan's input bound.
#[cfg(test)]
pub fn titan_chunks(text: &str) -> Vec<String> {
    assert!(!text.trim().is_empty());
    let mut chunks = Vec::new();
    let mut start = 0;
    for (index, character) in text.char_indices() {
        if index + character.len_utf8() - start > TITAN_MAX_CHUNK_BYTES {
            chunks.push(text[start..index].to_owned());
            start = index;
        }
    }
    chunks.push(text[start..].to_owned());
    chunks
}

fn register_space(path: &Path, space: &Space, now: i64) -> Result<(), Error> {
    db(path)?.execute(
        "INSERT INTO embedding_spaces(
           id,backend,model,dimensions,normalize,revision,model_sha256,tokenizer_sha256,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT(id) DO NOTHING",
        (
            &space.id,
            &space.backend,
            &space.model,
            i64::try_from(space.dimensions)?,
            space.normalize,
            &space.revision,
            &space.model_sha256,
            &space.tokenizer_sha256,
            now,
        ),
    )?;
    Ok(())
}

fn pending(
    path: &Path,
    space: &Space,
    now: i64,
    limit: usize,
) -> Result<Vec<(Vec<u8>, String)>, Error> {
    let conn = db(path)?;
    let mut statement = conn.prepare(
        "SELECT e.id,e.content
         FROM events e
         JOIN reader_events r ON r.event_id=e.id
         WHERE e.kind=1 AND e.created_at BETWEEN ?1 AND ?2 AND trim(e.content)!=''
           AND NOT EXISTS (SELECT 1 FROM policy_exclusions x WHERE x.event_id=lower(hex(e.id)))
           AND lower(hex(e.pubkey)) NOT IN (
             SELECT m.value FROM moderation_lists l,json_each(l.members_json) m
             WHERE l.identifier='nsfw' AND m.type='text')
           AND NOT EXISTS (
             SELECT 1 FROM post_embeddings v WHERE v.event_id=e.id AND v.space_id=?3)
         ORDER BY e.id LIMIT ?4",
    )?;
    let rows = statement.query_map(
        (now - WINDOW, now, &space.id, i64::try_from(limit)?),
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    rows.collect::<Result<_, _>>().map_err(Into::into)
}

fn request_status(
    conn: &Connection,
    event_id: &[u8],
    chunk_index: usize,
    space: &Space,
) -> Result<Option<String>, Error> {
    conn.query_row(
        "SELECT status FROM embedding_requests
         WHERE event_id=?1 AND chunk_index=?2 AND space_id=?3",
        (event_id, i64::try_from(chunk_index)?, &space.id),
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn reserve(
    path: &Path,
    event_id: &[u8],
    chunk_index: usize,
    input_bytes: usize,
    space: &Space,
    budget: Budget,
    now: i64,
) -> Result<Option<i64>, Error> {
    let mut conn = db(path)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if let Some(status) = request_status(&tx, event_id, chunk_index, space)? {
        return match status.as_str() {
            "succeeded" => Ok(None),
            "reserved" | "uncertain" => Err(format!(
                "Embedding request {chunk_index} has uncertain prior billing; refusing to retry"
            )
            .into()),
            _ => unreachable!(),
        };
    }
    // Reserve before transport; uncertain requests still count because billing may have occurred. -- Pi/gpt-5.6-sol
    let reserved = i64::try_from(input_bytes)? * space.price_nusd_per_token;
    let total: i64 = tx.query_row(
        "SELECT coalesce(sum(CASE status WHEN 'succeeded' THEN actual_nusd ELSE reserved_nusd END),0)
         FROM embedding_requests",
        [],
        |row| row.get(0),
    )?;
    let monthly: i64 = tx.query_row(
        "SELECT coalesce(sum(CASE status WHEN 'succeeded' THEN actual_nusd ELSE reserved_nusd END),0)
         FROM embedding_requests
         WHERE requested_at>=unixepoch(?1,'unixepoch','start of month')",
        [now],
        |row| row.get(0),
    )?;
    if total + reserved > budget.total_nusd || monthly + reserved > budget.monthly_nusd {
        return Err("Embedding budget exhausted before request reservation".into());
    }
    tx.execute(
        "INSERT INTO embedding_requests(
           event_id,chunk_index,space_id,requested_at,reserved_nusd,status)
         VALUES(?1,?2,?3,?4,?5,'reserved')",
        (
            event_id,
            i64::try_from(chunk_index)?,
            &space.id,
            now,
            reserved,
        ),
    )?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(Some(id))
}

fn vector_bytes(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn decode_vector(bytes: &[u8], dimensions: usize) -> Result<Vec<f32>, Error> {
    let (values, remainder) = bytes.as_chunks::<4>();
    if !remainder.is_empty() || values.len() != dimensions {
        return Err("Stored embedding has invalid dimensions".into());
    }
    Ok(values.iter().map(|raw| f32::from_le_bytes(*raw)).collect())
}

fn validate(output: &Output, space: &Space) -> Result<(), Error> {
    if output.vector.len() != space.dimensions
        || output.input_tokens < 0
        || output.vector.iter().any(|value| !value.is_finite())
    {
        return Err("Invalid embedding service response".into());
    }
    Ok(())
}

struct Completion<'a> {
    request_id: i64,
    event_id: &'a [u8],
    chunk_index: usize,
    chunk_count: usize,
    input_bytes: usize,
    output: &'a Output,
    now: i64,
}

fn complete_request(path: &Path, space: &Space, completion: Completion<'_>) -> Result<(), Error> {
    let Completion {
        request_id,
        event_id,
        chunk_index,
        chunk_count,
        input_bytes,
        output,
        now,
    } = completion;
    validate(output, space)?;
    let actual_nusd = output.input_tokens * space.price_nusd_per_token;
    let mut conn = db(path)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let reserved: i64 = tx.query_row(
        "SELECT reserved_nusd FROM embedding_requests WHERE id=?1 AND status='reserved'",
        [request_id],
        |row| row.get(0),
    )?;
    if actual_nusd > reserved {
        tx.execute(
            "UPDATE embedding_requests
             SET status='uncertain',actual_tokens=?1,actual_nusd=?2,error='actual cost exceeded reservation'
             WHERE id=?3",
            (output.input_tokens, actual_nusd, request_id),
        )?;
        tx.commit()?;
        return Err("Embedding service token count exceeded conservative reservation".into());
    }
    tx.execute(
        "INSERT INTO embedding_chunks(
           event_id,chunk_index,chunk_count,space_id,dimensions,input_bytes,input_tokens,vector,embedded_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        (
            event_id,
            i64::try_from(chunk_index)?,
            i64::try_from(chunk_count)?,
            &space.id,
            i64::try_from(space.dimensions)?,
            i64::try_from(input_bytes)?,
            output.input_tokens,
            vector_bytes(&output.vector),
            now,
        ),
    )?;
    tx.execute(
        "UPDATE embedding_requests
         SET status='succeeded',actual_tokens=?1,actual_nusd=?2 WHERE id=?3",
        (output.input_tokens, actual_nusd, request_id),
    )?;
    tx.commit()?;
    Ok(())
}

fn mark_uncertain(path: &Path, request_id: i64, error: &str) -> Result<(), Error> {
    db(path)?.execute(
        "UPDATE embedding_requests SET status='uncertain',error=?1 WHERE id=?2",
        (error, request_id),
    )?;
    Ok(())
}

fn finalize(
    path: &Path,
    event_id: &[u8],
    chunk_count: usize,
    space: &Space,
    now: i64,
) -> Result<(), Error> {
    let mut conn = db(path)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut statement = tx.prepare(
        "SELECT vector,input_tokens FROM embedding_chunks
         WHERE event_id=?1 AND space_id=?2 ORDER BY chunk_index",
    )?;
    let rows: Vec<(Vec<u8>, i64)> = statement
        .query_map((event_id, &space.id), |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    drop(statement);
    if rows.len() != chunk_count {
        return Err(format!(
            "Embedding chunks incomplete: stored={} expected={chunk_count}",
            rows.len()
        )
        .into());
    }
    let mut mean = vec![0.0_f32; space.dimensions];
    let mut tokens = 0_i64;
    for (bytes, input_tokens) in &rows {
        for (mean_value, chunk_value) in
            mean.iter_mut().zip(decode_vector(bytes, space.dimensions)?)
        {
            *mean_value += chunk_value;
        }
        tokens += input_tokens;
    }
    let norm = mean.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err("Embedding aggregate has zero or invalid norm".into());
    }
    for value in &mut mean {
        *value /= norm;
    }
    let cost: i64 = tx.query_row(
        "SELECT sum(actual_nusd) FROM embedding_requests
         WHERE event_id=?1 AND space_id=?2 AND status='succeeded'",
        (event_id, &space.id),
        |row| row.get(0),
    )?;
    tx.execute(
        "INSERT INTO post_embeddings(
           event_id,space_id,dimensions,chunk_count,input_tokens,cost_nusd,vector,embedded_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        (
            event_id,
            &space.id,
            i64::try_from(space.dimensions)?,
            i64::try_from(chunk_count)?,
            tokens,
            cost,
            vector_bytes(&mean),
            now,
        ),
    )?;
    tx.commit()?;
    Ok(())
}

/// Embeds eligible posts after collection scans have drained all admitted SDK writes.
///
/// Run this in the collector sequence, not as an independent SQLite writer. No transaction spans
/// the model call. Titan settings follow the AWS request contract:
/// <https://docs.aws.amazon.com/bedrock/latest/userguide/model-parameters-titan-embed-text.html>.
/// -- Pi/gpt-5.6-sol
pub async fn embed_pending(
    path: &Path,
    transport: &impl Transport,
    budget: Budget,
    now: i64,
    limit: usize,
) -> Result<usize, Error> {
    let space = transport.space();
    register_space(path, space, now)?;
    let posts = pending(path, space, now, limit)?;
    for (event_id, text) in &posts {
        let text_chunks = transport.split(text)?;
        for (chunk_index, text_chunk) in text_chunks.iter().enumerate() {
            let Some(request_id) = reserve(
                path,
                event_id,
                chunk_index,
                text_chunk.len(),
                space,
                budget,
                now,
            )?
            else {
                continue;
            };
            // No SQLite transaction spans the model call. -- Pi/gpt-5.6-sol
            let output = match transport.embed(text_chunk).await {
                Ok(output) => output,
                Err(error) => {
                    mark_uncertain(path, request_id, &error.to_string())?;
                    return Err(error);
                }
            };
            complete_request(
                path,
                space,
                Completion {
                    request_id,
                    event_id,
                    chunk_index,
                    chunk_count: text_chunks.len(),
                    input_bytes: text_chunk.len(),
                    output: &output,
                    now,
                },
            )?;
        }
        finalize(path, event_id, text_chunks.len(), space, now)?;
    }
    Ok(posts.len())
}

/// Loads one event vector from the model's exact space.
pub fn event_vector(
    path: &Path,
    space: &Space,
    event_id: &[u8],
) -> Result<Option<Vec<f32>>, Error> {
    let conn = db(path)?;
    let bytes: Option<Vec<u8>> = conn
        .query_row(
            "SELECT vector FROM post_embeddings WHERE event_id=?1 AND space_id=?2",
            (event_id, &space.id),
            |row| row.get(0),
        )
        .optional()?;
    bytes
        .map(|value| decode_vector(&value, space.dimensions))
        .transpose()
}

/// Returns cosine-ranked event IDs from one exact vector space.
pub fn nearest(
    path: &Path,
    space: &Space,
    query: &[f32],
    exclude: Option<&[u8]>,
    limit: usize,
) -> Result<Vec<(Vec<u8>, f32)>, Error> {
    if query.len() != space.dimensions {
        return Err("Query embedding has invalid dimensions".into());
    }
    let conn = db(path)?;
    let mut statement =
        conn.prepare("SELECT event_id,vector FROM post_embeddings WHERE space_id=?1")?;
    let mut scored = statement
        .query_map([&space.id], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(event_id, _)| exclude != Some(event_id))
        .map(|(event_id, bytes)| {
            let vector = decode_vector(&bytes, space.dimensions)?;
            let score: f32 = query.iter().zip(vector).map(|(a, b)| a * b).sum();
            Ok((event_id, score))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    scored.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    scored.truncate(limit);
    Ok(scored)
}
