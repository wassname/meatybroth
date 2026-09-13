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
    sync::{LazyLock, Mutex},
};

pub const TITAN_MODEL: &str = "amazon.titan-embed-text-v2:0";
pub const TITAN_DIMENSIONS: usize = 512;
// Titan Text Embeddings V2 accepts at most 8,192 input tokens. https://docs.aws.amazon.com/bedrock/latest/userguide/titan-embedding-models.html -- Pi/gpt-5.6-sol
const TITAN_MAX_INPUT_TOKENS: usize = 8192;
pub const MINILM_MODEL: &str = "sentence-transformers/all-MiniLM-L6-v2";
pub const MINILM_DIMENSIONS: usize = 384;
const TITAN_MAX_CHUNK_BYTES: usize = 8_000;
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
    fn concurrency(&self) -> usize {
        1
    }
    fn embed<'a>(
        &'a self,
        text: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Output, Error>> + Send + 'a>>;
}

/// Synchronous query interface shared by native inference and HTTP test doubles.
pub trait SemanticModel: Send + Sync {
    fn vector_space(&self) -> &Space;
    fn embed_text(&self, text: &str) -> Result<Output, Error>;
}

/// Native AWS SDK transport for the explicitly approved one-off Titan backfill.
pub struct Bedrock {
    client: aws_sdk_bedrockruntime::Client,
    space: Space,
}

impl Bedrock {
    pub async fn new(region: &str) -> Self {
        let timeout = aws_config::timeout::TimeoutConfig::builder()
            .operation_timeout(std::time::Duration::from_secs(30))
            .operation_attempt_timeout(std::time::Duration::from_secs(30))
            .build();
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_sdk_bedrockruntime::config::Region::new(
                region.to_owned(),
            ))
            .retry_config(aws_config::retry::RetryConfig::standard().with_max_attempts(1))
            .timeout_config(timeout)
            .load()
            .await;
        Self {
            client: aws_sdk_bedrockruntime::Client::new(&config),
            space: Space::titan_v2(),
        }
    }
}

impl Transport for Bedrock {
    fn space(&self) -> &Space {
        &self.space
    }

    fn split(&self, text: &str) -> Result<Vec<String>, Error> {
        Ok(titan_chunks(text))
    }

    fn concurrency(&self) -> usize {
        4
    }

    fn embed<'a>(
        &'a self,
        text: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Output, Error>> + Send + 'a>> {
        Box::pin(async move {
            let body = serde_json::json!({
                "inputText": text,
                "dimensions": TITAN_DIMENSIONS,
                "normalize": true,
            });
            let result = self
                .client
                .invoke_model()
                .model_id(TITAN_MODEL)
                .content_type("application/json")
                .accept("application/json")
                .body(aws_sdk_bedrockruntime::primitives::Blob::new(
                    serde_json::to_vec(&body)?,
                ))
                .send()
                .await;
            let response = result.map_err(|error| {
                let stage = if error.to_string().contains("credentials") {
                    "preflight"
                } else {
                    "uncertain"
                };
                format!("{stage}: Titan InvokeModel failed: {error}")
            })?;
            let response: serde_json::Value = serde_json::from_slice(response.body.as_ref())?;
            let vector = response["embedding"]
                .as_array()
                .ok_or("Titan response omitted embedding")?
                .iter()
                .map(|value| {
                    value
                        .as_f64()
                        .map(|number| number as f32)
                        .ok_or("Titan returned a non-number")
                })
                .collect::<Result<Vec<_>, _>>()?;
            let input_tokens = response["inputTextTokenCount"]
                .as_i64()
                .ok_or("Titan response omitted inputTextTokenCount")?;
            Ok(Output {
                vector,
                input_tokens,
            })
        })
    }
}

/// Native ONNX MiniLM inference with tokenizer-bound chunking.
pub struct MiniLm {
    model: Mutex<TextEmbedding>,
    space: Space,
}

impl MiniLm {
    /// Resolves Hugging Face `refs/main` and records its revision and loaded-file hashes.
    ///
    /// A revision or hash change creates a new space ID, so old vectors are never reused or mixed.
    /// -- Pi/gpt-5.6-sol
    pub fn open(cache_dir: &Path, intra_threads: usize) -> Result<Self, Error> {
        let model = TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_cache_dir(cache_dir.to_path_buf())
                .with_max_length(MINILM_MAX_SEQUENCE_TOKENS)
                .with_intra_threads(intra_threads)
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
        let mut untruncated = model.tokenizer.clone();
        untruncated.with_truncation(None)?;
        if untruncated.encode(text, false)?.len() > MINILM_CONTENT_TOKENS {
            return Err("Meaning query exceeds MiniLM's 254-content-token window".into());
        }
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

impl SemanticModel for MiniLm {
    fn vector_space(&self) -> &Space {
        self.vector_space()
    }

    fn embed_text(&self, text: &str) -> Result<Output, Error> {
        self.embed_text(text)
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
        let mut pending: std::collections::VecDeque<_> = chunks.into();
        let mut verified = Vec::new();
        while let Some(chunk) = pending.pop_front() {
            if tokenizer.encode(chunk.as_str(), false)?.len() <= MINILM_CONTENT_TOKENS {
                verified.push(chunk);
                continue;
            }
            let midpoint = chunk.len() / 2;
            let split = chunk
                .char_indices()
                .map(|(index, _)| index)
                .min_by_key(|index| index.abs_diff(midpoint))
                .filter(|index| *index > 0)
                .ok_or("MiniLM could not split an over-window token span")?;
            let right = chunk[split..].to_owned();
            let left = chunk[..split].to_owned();
            pending.push_front(right);
            pending.push_front(left);
        }
        if verified.concat() != text {
            return Err("Verified MiniLM chunks did not preserve the full input".into());
        }
        Ok(verified)
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
    conn.pragma_update(None, "cache_size", -65_536)?;
    conn.pragma_update(None, "mmap_size", 268_435_456)?;
    conn.execute_batch("PRAGMA foreign_keys=ON")?;
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

/// Loads the newest recorded space for one explicit backend.
pub fn space_by_backend(path: &Path, backend: &str) -> Result<Option<Space>, Error> {
    db(path)?
        .query_row(
            "SELECT id,backend,model,dimensions,normalize,revision,model_sha256,tokenizer_sha256
             FROM embedding_spaces WHERE backend=?1 ORDER BY created_at DESC LIMIT 1",
            [backend],
            |row| {
                let backend: String = row.get(1)?;
                Ok(Space {
                    id: row.get(0)?,
                    model: row.get(2)?,
                    dimensions: usize::try_from(row.get::<_, i64>(3)?)
                        .expect("embedding dimensions must be nonnegative"),
                    normalize: row.get(4)?,
                    revision: row.get(5)?,
                    model_sha256: row.get(6)?,
                    tokenizer_sha256: row.get(7)?,
                    price_nusd_per_token: if backend == "bedrock" {
                        TITAN_PRICE_NUSD_PER_TOKEN
                    } else {
                        0
                    },
                    backend,
                })
            },
        )
        .optional()
        .map_err(Into::into)
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
         JOIN reader_post_events r ON r.event_id=e.id
         WHERE e.kind=1 AND e.created_at BETWEEN ?1 AND ?2 AND trim(e.content)!=''
           AND NOT EXISTS (
             SELECT 1 FROM post_embeddings v WHERE v.event_id=e.id AND v.space_id=?3)
           AND NOT EXISTS (
             SELECT 1 FROM embedding_requests request
             WHERE request.event_id=e.id AND request.space_id=?3
               AND request.status IN ('reserved','uncertain'))
         ORDER BY CASE WHEN ?4='bedrock' THEN e.id END,
                  CASE WHEN ?4!='bedrock' THEN e.created_at END DESC,e.id LIMIT ?5",
    )?;
    let rows = statement.query_map(
        (
            now - WINDOW,
            now,
            &space.id,
            &space.backend,
            i64::try_from(limit)?,
        ),
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
    let reserved_tokens = if space.backend == "bedrock" {
        TITAN_MAX_INPUT_TOKENS
    } else {
        input_bytes
    };
    let reserved = i64::try_from(reserved_tokens)? * space.price_nusd_per_token;
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

fn archive_preflight_failure(
    path: &Path,
    request_id: i64,
    error: &str,
    now: i64,
) -> Result<(), Error> {
    let mut conn = db(path)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO embedding_preflight_failures(
           request_id,event_id,chunk_index,space_id,requested_at,reserved_nusd,error,archived_at)
         SELECT id,event_id,chunk_index,space_id,requested_at,reserved_nusd,?1,?2
         FROM embedding_requests WHERE id=?3 AND status='reserved'",
        (error, now, request_id),
    )?;
    tx.execute(
        "DELETE FROM embedding_requests WHERE id=?1 AND status='reserved'",
        [request_id],
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

async fn embed_event(
    path: &Path,
    transport: &(impl Transport + ?Sized),
    budget: Budget,
    now: i64,
    event_id: &[u8],
    text: &str,
) -> Result<(), Error> {
    let space = transport.space();
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
        let output = match transport.embed(text_chunk).await {
            Ok(output) => output,
            Err(error) => {
                let message = error.to_string();
                if message.starts_with("preflight:") {
                    archive_preflight_failure(path, request_id, &message, now)?;
                } else {
                    mark_uncertain(path, request_id, &message)?;
                }
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
        tokio::task::yield_now().await;
    }
    finalize(path, event_id, text_chunks.len(), space, now)?;
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
    transport: &(impl Transport + ?Sized),
    budget: Budget,
    now: i64,
    limit: usize,
) -> Result<usize, Error> {
    let space = transport.space();
    register_space(path, space, now)?;
    let posts = pending(path, space, now, limit)?;
    let mut embedded = 0;
    for window in posts.chunks(transport.concurrency()) {
        // SQLite sections finish synchronously; only provider awaits overlap. -- Pi/gpt-5.6-sol
        let results = futures_util::future::join_all(
            window
                .iter()
                .map(|(event_id, text)| embed_event(path, transport, budget, now, event_id, text)),
        )
        .await;
        let mut first_error = None;
        for result in results {
            match result {
                Ok(()) => embedded += 1,
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
    }
    Ok(embedded)
}

/// Caches one exact text query under the same pre-call budget ledger.
pub async fn cache_query(
    path: &Path,
    transport: &(impl Transport + ?Sized),
    budget: Budget,
    query: &str,
    now: i64,
) -> Result<bool, Error> {
    let space = transport.space();
    register_space(path, space, now)?;
    if db(path)?
        .query_row(
            "SELECT 1 FROM embedding_queries WHERE space_id=?1 AND query=?2",
            (&space.id, query),
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        return Ok(false);
    }
    let event_id = Sha256::digest(format!("query:{query}").as_bytes());
    let request_id = reserve(path, &event_id, 0, query.len(), space, budget, now)?
        .ok_or("Query request succeeded without a cached vector")?;
    let output = match transport.embed(query).await {
        Ok(output) => output,
        Err(error) => {
            let message = error.to_string();
            if message.starts_with("preflight:") {
                archive_preflight_failure(path, request_id, &message, now)?;
            } else {
                mark_uncertain(path, request_id, &message)?;
            }
            return Err(error);
        }
    };
    validate(&output, space)?;
    let actual_nusd = output.input_tokens * space.price_nusd_per_token;
    let mut conn = db(path)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let reserved: i64 = tx.query_row(
        "SELECT reserved_nusd FROM embedding_requests WHERE id=?1 AND status='reserved'",
        [request_id],
        |row| row.get(0),
    )?;
    if actual_nusd > reserved {
        return Err("Query token count exceeded conservative reservation".into());
    }
    tx.execute(
        "INSERT INTO embedding_queries(space_id,query,vector,input_tokens,cost_nusd,embedded_at)
         VALUES(?1,?2,?3,?4,?5,?6)",
        (
            &space.id,
            query,
            vector_bytes(&output.vector),
            output.input_tokens,
            actual_nusd,
            now,
        ),
    )?;
    tx.execute(
        "UPDATE embedding_requests SET status='succeeded',actual_tokens=?1,actual_nusd=?2
         WHERE id=?3",
        (output.input_tokens, actual_nusd, request_id),
    )?;
    tx.commit()?;
    Ok(true)
}

/// Loads a previously paid exact query without invoking a provider.
pub fn cached_query(path: &Path, space: &Space, query: &str) -> Result<Option<Vec<f32>>, Error> {
    let bytes: Option<Vec<u8>> = db(path)?
        .query_row(
            "SELECT vector FROM embedding_queries WHERE space_id=?1 AND query=?2",
            (&space.id, query),
            |row| row.get(0),
        )
        .optional()?;
    bytes
        .map(|value| decode_vector(&value, space.dimensions))
        .transpose()
}

/// Stored-vector coverage and request-ledger totals for one provenance-separated space.
#[derive(Debug, serde::Serialize)]
pub struct CacheStatus {
    pub backend: String,
    pub model: String,
    pub dimensions: i64,
    pub revision: String,
    pub model_sha256: String,
    pub tokenizer_sha256: String,
    pub vectors: i64,
    pub eligible_vectors: i64,
    pub pending: i64,
    pub requests: i64,
    pub tokens: i64,
    pub cost_nusd: i64,
    pub uncertain: i64,
}

/// Summarizes current eligibility separately from durable all-time provider accounting.
pub fn cache_status(db: &Connection, now: i64) -> Result<Vec<CacheStatus>, Error> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='embedding_spaces')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(Vec::new());
    }
    let mut statement = db.prepare(
        "WITH eligible AS MATERIALIZED (
           SELECT event.id
           FROM events event
           JOIN reader_post_events reader ON reader.event_id=event.id
           WHERE event.kind=1 AND event.created_at BETWEEN ?1 AND ?2
         )
         SELECT
           space.backend,
           space.model,
           space.dimensions,
           space.revision,
           space.model_sha256,
           space.tokenizer_sha256,
           (SELECT count(*) FROM post_embeddings post WHERE post.space_id=space.id),
           (SELECT count(*) FROM post_embeddings post
             JOIN eligible ON eligible.id=post.event_id WHERE post.space_id=space.id),
           (SELECT count(*) FROM eligible
             WHERE NOT EXISTS (SELECT 1 FROM post_embeddings post
               WHERE post.event_id=eligible.id AND post.space_id=space.id)),
           (SELECT count(*) FROM embedding_requests request
             WHERE request.space_id=space.id AND request.status='succeeded'),
           (SELECT coalesce(sum(request.actual_tokens),0) FROM embedding_requests request
             WHERE request.space_id=space.id AND request.status='succeeded'),
           (SELECT coalesce(sum(request.actual_nusd),0) FROM embedding_requests request
             WHERE request.space_id=space.id AND request.status='succeeded'),
           (SELECT count(*) FROM embedding_requests request
             WHERE request.space_id=space.id AND request.status='uncertain')
         FROM embedding_spaces space
         ORDER BY space.backend,space.created_at DESC",
    )?;
    let statuses = statement
        .query_map((now - WINDOW, now), |row| {
            Ok(CacheStatus {
                backend: row.get(0)?,
                model: row.get(1)?,
                dimensions: row.get(2)?,
                revision: row.get(3)?,
                model_sha256: row.get(4)?,
                tokenizer_sha256: row.get(5)?,
                vectors: row.get(6)?,
                eligible_vectors: row.get(7)?,
                pending: row.get(8)?,
                requests: row.get(9)?,
                tokens: row.get(10)?,
                cost_nusd: row.get(11)?,
                uncertain: row.get(12)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(statuses)
}

/// One keyword-labelled cluster in an exact embedding space.
#[derive(Debug, serde::Serialize)]
pub struct Topic {
    pub id: i64,
    pub label: String,
    pub post_count: i64,
}

fn normalize(vector: &mut [f32]) -> Result<(), Error> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err("Topic centroid has zero or invalid norm".into());
    }
    for value in vector {
        *value /= norm;
    }
    Ok(())
}

fn topic_label(texts: &[&str]) -> String {
    // stopwords-iso English list, MIT, pinned at ccc8898. -- Pi/gpt-5.6-sol
    // https://github.com/stopwords-iso/stopwords-en/tree/ccc8898188850d8fb019d5f69c14a6635c3bd115
    static STOP: LazyLock<std::collections::HashSet<&'static str>> =
        LazyLock::new(|| include_str!("data/english-stopwords.txt").lines().collect());
    let urls = regex::Regex::new(r"(?i)https?://\S+").unwrap();
    let words = regex::Regex::new(r"(?i)[\p{L}\p{N}][\p{L}\p{N}_-]{2,}").unwrap();
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for text in texts {
        let without_urls = urls.replace_all(text, " ");
        let unique: std::collections::HashSet<_> = words
            .find_iter(&without_urls)
            .map(|word| word.as_str().to_lowercase())
            .filter(|word| {
                word.is_ascii()
                    && word
                        .chars()
                        .any(|character| character.is_ascii_alphabetic())
                    && word.chars().count() <= 32
                    && !STOP.contains(word.as_str())
                    && !word.starts_with("http")
            })
            .collect();
        for word in unique {
            *counts.entry(word).or_default() += 1;
        }
    }
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let minimum_support = 2.max(texts.len().div_ceil(5));
    let terms: Vec<_> = counts
        .into_iter()
        .filter(|(_, support)| *support >= minimum_support)
        .take(3)
        .map(|(word, _)| word)
        .collect();
    if terms.len() < 2 {
        "mixed".into()
    } else {
        terms.join(" · ")
    }
}

/// Recomputes deterministic cosine clusters and labels from current post text.
pub fn cluster_topics(path: &Path, space: &Space, now: i64) -> Result<usize, Error> {
    let mut conn = db(path)?;
    let mut statement = conn.prepare(
        "SELECT embedding.event_id,embedding.vector,event.content
         FROM post_embeddings embedding
         JOIN events event ON event.id=embedding.event_id
         JOIN reader_post_events reader ON reader.event_id=event.id
         WHERE embedding.space_id=?1 AND event.created_at BETWEEN ?2 AND ?3
         ORDER BY embedding.event_id",
    )?;
    let rows: Vec<(Vec<u8>, Vec<f32>, String)> = statement
        .query_map((&space.id, now - WINDOW, now), |row| {
            Ok((
                row.get(0)?,
                decode_vector(&row.get::<_, Vec<u8>>(1)?, space.dimensions)
                    .map_err(rusqlite::Error::ToSqlConversionFailure)?,
                row.get(2)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    drop(statement);
    if rows.is_empty() {
        return Ok(0);
    }
    let cluster_count = ((rows.len() as f64 / 25.0).sqrt().round() as usize)
        .clamp(6, 16)
        .min(rows.len());
    let mut centroids = vec![rows[0].1.clone()];
    while centroids.len() < cluster_count {
        let next = rows
            .iter()
            .enumerate()
            .min_by(|left, right| {
                let nearest = |vector: &[f32]| {
                    centroids
                        .iter()
                        .map(|center| vector.iter().zip(center).map(|(a, b)| a * b).sum::<f32>())
                        .max_by(f32::total_cmp)
                        .unwrap()
                };
                nearest(&left.1 .1).total_cmp(&nearest(&right.1 .1))
            })
            .unwrap()
            .0;
        centroids.push(rows[next].1.clone());
    }
    let mut assignments = vec![0; rows.len()];
    for _ in 0..8 {
        for (assignment, (_, vector, _)) in assignments.iter_mut().zip(&rows) {
            *assignment = centroids
                .iter()
                .enumerate()
                .max_by(|left, right| {
                    let score =
                        |center: &[f32]| vector.iter().zip(center).map(|(a, b)| a * b).sum::<f32>();
                    score(left.1).total_cmp(&score(right.1))
                })
                .unwrap()
                .0;
        }
        let mut sums = vec![vec![0.0_f32; space.dimensions]; cluster_count];
        let mut sizes = vec![0; cluster_count];
        for (assignment, (_, vector, _)) in assignments.iter().zip(&rows) {
            sizes[*assignment] += 1;
            for (sum, value) in sums[*assignment].iter_mut().zip(vector) {
                *sum += value;
            }
        }
        for (index, sum) in sums.iter_mut().enumerate() {
            if sizes[index] > 0 {
                normalize(sum)?;
                centroids[index].clone_from(sum);
            }
        }
    }
    let labels: Vec<_> = (0..cluster_count)
        .map(|cluster| {
            topic_label(
                &assignments
                    .iter()
                    .zip(&rows)
                    .filter(|(assignment, _)| **assignment == cluster)
                    .map(|(_, (_, _, text))| text.as_str())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute(
        "DELETE FROM embedding_topics WHERE space_id=?1",
        [&space.id],
    )?;
    let mut stored_topics = 0;
    for (topic_id, (label, centroid)) in labels.iter().zip(&centroids).enumerate() {
        let post_count = assignments
            .iter()
            .filter(|value| **value == topic_id)
            .count();
        if post_count == 0 {
            continue;
        }
        tx.execute(
            "INSERT INTO embedding_topics(space_id,topic_id,label,post_count,centroid,created_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            (
                &space.id,
                i64::try_from(topic_id)?,
                label,
                i64::try_from(post_count)?,
                vector_bytes(centroid),
                now,
            ),
        )?;
        stored_topics += 1;
    }
    for ((event_id, _, _), topic_id) in rows.iter().zip(assignments) {
        tx.execute(
            "INSERT INTO post_topics(event_id,space_id,topic_id) VALUES(?1,?2,?3)",
            (event_id, &space.id, i64::try_from(topic_id)?),
        )?;
    }
    tx.commit()?;
    Ok(stored_topics)
}

/// Lists keyword-labelled topics largest first.
pub fn topics(path: &Path, space: &Space) -> Result<Vec<Topic>, Error> {
    let conn = db(path)?;
    let mut statement = conn.prepare(
        "SELECT topic_id,label,post_count FROM embedding_topics
         WHERE space_id=?1 ORDER BY post_count DESC,topic_id",
    )?;
    let topics = statement
        .query_map([&space.id], |row| {
            Ok(Topic {
                id: row.get(0)?,
                label: row.get(1)?,
                post_count: row.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()
        .map_err(Into::into);
    topics
}

/// Returns one topic's event IDs in newest-first order.
pub fn topic_events(
    path: &Path,
    space: &Space,
    topic_id: i64,
    now: i64,
    limit: usize,
    offset: usize,
) -> Result<Vec<Vec<u8>>, Error> {
    let conn = db(path)?;
    let mut statement = conn.prepare(
        "SELECT topic.event_id FROM post_topics topic
         JOIN events event ON event.id=topic.event_id
         JOIN reader_post_events reader ON reader.event_id=event.id
         WHERE topic.space_id=?1 AND topic.topic_id=?2 AND event.created_at BETWEEN ?3 AND ?4
         ORDER BY event.created_at DESC,event.id LIMIT ?5 OFFSET ?6",
    )?;
    let events = statement
        .query_map(
            (
                &space.id,
                topic_id,
                now - WINDOW,
                now,
                i64::try_from(limit)?,
                i64::try_from(offset)?,
            ),
            |row| row.get(0),
        )?
        .collect::<Result<_, _>>()
        .map_err(Into::into);
    events
}

/// Counts cached post vectors in one exact space.
pub fn vector_count(path: &Path, space: &Space) -> Result<i64, Error> {
    Ok(db(path)?.query_row(
        "SELECT count(*) FROM post_embeddings WHERE space_id=?1",
        [&space.id],
        |row| row.get(0),
    )?)
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
    now: i64,
    limit: usize,
) -> Result<Vec<(Vec<u8>, f32)>, Error> {
    if query.len() != space.dimensions {
        return Err("Query embedding has invalid dimensions".into());
    }
    let conn = db(path)?;
    let mut statement = conn.prepare(
        "SELECT embedding.event_id,embedding.vector
         FROM post_embeddings embedding
         JOIN events event ON event.id=embedding.event_id
         JOIN reader_post_events reader ON reader.event_id=event.id
         WHERE embedding.space_id=?1 AND event.created_at BETWEEN ?2 AND ?3",
    )?;
    let mut scored = statement
        .query_map((&space.id, now - WINDOW, now), |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(event_id, _)| exclude != Some(event_id))
        .map(|(event_id, bytes)| {
            let (values, remainder) = bytes.as_chunks::<4>();
            if !remainder.is_empty() || values.len() != space.dimensions {
                return Err("Stored embedding has invalid dimensions".into());
            }
            let score: f32 = query
                .iter()
                .zip(values)
                .map(|(a, raw)| a * f32::from_le_bytes(*raw))
                .sum();
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

#[cfg(test)]
mod label_tests {
    use super::topic_label;

    #[test]
    fn labels_remove_english_fragments_and_mark_heterogeneous_clusters() {
        assert_eq!(
            topic_label(&["good year all", "them because don't"]),
            "mixed"
        );
        assert_eq!(
            topic_label(&["rust vector search", "rust vector index", "rust vector"]),
            "rust · vector"
        );
    }
}
