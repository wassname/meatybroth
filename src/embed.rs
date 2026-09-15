//! Builds local or Bedrock embeddings for admitted notes in the reader's SQLite file.
//!
//! Each vector space has model, revision, tokenizer, and content-hash provenance. Request rows are
//! the durable cost ledger; chunk and post vectors belong to SDK events. -- Pi/gpt-5.6-sol

use crate::{queries::WINDOW, Error};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use linfa::traits::Transformer;
use linfa_clustering::Dbscan;
use linfa_nn::{
    distance::{Distance, L2Dist},
    BallTree, NearestNeighbour,
};
use ndarray::{Array2, ArrayView1};
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
    fn epoch_seconds(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
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

/// Native AWS SDK transport for Titan backfill and authorized continuous production.
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
                format!("{stage}: Titan InvokeModel failed: {error:?}")
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

pub fn mark_admissions(
    path: &Path,
    event_ids: &[Vec<u8>],
    admitted_at: i64,
    live: bool,
) -> Result<(), Error> {
    let mut conn = db(path)?;
    let transaction = conn.transaction()?;
    for event_id in event_ids {
        transaction.execute(
            "INSERT INTO embedding_admissions(event_id,admitted_at,live) VALUES(?1,?2,?3)
             ON CONFLICT(event_id) DO UPDATE SET
               admitted_at=CASE
                 WHEN embedding_admissions.live=0 AND excluded.live=1 THEN excluded.admitted_at
                 ELSE min(embedding_admissions.admitted_at,excluded.admitted_at)
               END,
               live=max(embedding_admissions.live,excluded.live)",
            rusqlite::params![event_id, admitted_at, live],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn pending(
    path: &Path,
    space: &Space,
    now: i64,
    limit: usize,
    recent_first: bool,
) -> Result<Vec<(Vec<u8>, String)>, Error> {
    let conn = db(path)?;
    let mut statement = conn.prepare(
        "SELECT e.id,e.content
         FROM events e
         JOIN reader_post_events r ON r.event_id=e.id
         LEFT JOIN embedding_admissions admission ON admission.event_id=e.id
         WHERE e.kind=1 AND e.created_at BETWEEN ?1 AND ?2 AND trim(e.content)!=''
           AND NOT EXISTS (
             SELECT 1 FROM embedding_input_rejections rejection
             WHERE rejection.event_id=e.id AND rejection.space_id=?3)
           AND NOT EXISTS (
             SELECT 1 FROM post_embeddings v WHERE v.event_id=e.id AND v.space_id=?3)
           AND NOT EXISTS (
             SELECT 1 FROM embedding_requests request
             WHERE request.event_id=e.id AND request.space_id=?3
               AND request.status IN ('reserved','uncertain'))
         ORDER BY CASE WHEN ?4 AND coalesce(admission.live,0)=1 THEN 0 ELSE 1 END,
                  CASE WHEN ?4 AND coalesce(admission.live,0)=1 THEN admission.admitted_at END,
                  CASE WHEN NOT ?4 AND ?5='bedrock' THEN e.id END,
                  CASE WHEN NOT ?4 AND ?5!='bedrock' THEN e.created_at END DESC,e.id LIMIT ?6",
    )?;
    let rows = statement.query_map(
        (
            now - WINDOW,
            now,
            &space.id,
            recent_first,
            &space.backend,
            i64::try_from(limit)?,
        ),
        |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)),
    )?;
    let rows = rows.collect::<Result<Vec<_>, _>>()?;
    let rejected = rows
        .iter()
        .filter(|(_, text)| text.trim().is_empty())
        .map(|(event_id, _)| event_id)
        .collect::<Vec<_>>();
    if !rejected.is_empty() {
        let mut conn = db(path)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for event_id in rejected {
            tx.execute(
                "INSERT INTO embedding_input_rejections(event_id,space_id,rejected_at,reason)
                 VALUES(?1,?2,?3,'blank text')
                 ON CONFLICT(event_id,space_id) DO NOTHING",
                (event_id, &space.id, now),
            )?;
        }
        tx.commit()?;
    }
    Ok(rows
        .into_iter()
        .filter(|(_, text)| !text.trim().is_empty())
        .collect())
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

/// Nostr relay shutdowns never enter this transport; this exact HTTP/2 provider failure may follow dispatch.
pub(crate) fn is_http2_goaway(error: &Error) -> bool {
    let message = error.to_string();
    message.contains("HTTP/2 protocol error") && message.contains("GoAway(")
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

fn deadline_reached(transport: &(impl Transport + ?Sized), deadline: Option<u64>) -> bool {
    deadline.is_some_and(|deadline| transport.epoch_seconds() >= deadline)
}

async fn embed_event(
    path: &Path,
    transport: &(impl Transport + ?Sized),
    budget: Budget,
    deadline: Option<u64>,
    now: i64,
    event_id: &[u8],
    text: &str,
) -> Result<bool, Error> {
    let space = transport.space();
    let text_chunks = transport.split(text)?;
    for (chunk_index, text_chunk) in text_chunks.iter().enumerate() {
        if deadline_reached(transport, deadline) {
            return Ok(false);
        }
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
    Ok(true)
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
    let deadline = std::env::var("MEATYBROTH_EMBED_DEADLINE_EPOCH")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()?;
    embed_pending_until(path, transport, budget, deadline, now, limit, false).await
}

/// Embeds the newest eligible posts first so live collection does not wait behind a backfill.
pub async fn embed_recent_pending(
    path: &Path,
    transport: &(impl Transport + ?Sized),
    budget: Budget,
    now: i64,
    limit: usize,
) -> Result<usize, Error> {
    embed_pending_until(path, transport, budget, None, now, limit, true).await
}

pub(crate) async fn embed_pending_until(
    path: &Path,
    transport: &(impl Transport + ?Sized),
    budget: Budget,
    deadline: Option<u64>,
    now: i64,
    limit: usize,
    recent_first: bool,
) -> Result<usize, Error> {
    let space = transport.space();
    register_space(path, space, now)?;
    let posts = pending(path, space, now, limit, recent_first)?;
    let mut embedded = 0;
    for window in posts.chunks(transport.concurrency()) {
        if deadline_reached(transport, deadline) {
            break;
        }
        // SQLite sections finish synchronously; only provider awaits overlap. -- Pi/gpt-5.6-sol
        let results = futures_util::future::join_all(window.iter().map(|(event_id, text)| {
            embed_event(path, transport, budget, deadline, now, event_id, text)
        }))
        .await;
        let mut first_error = None;
        for result in results {
            match result {
                Ok(true) => embedded += 1,
                Ok(false) => {}
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
    pub rejected: i64,
    pub requests: i64,
    pub tokens: i64,
    pub cost_nusd: i64,
    pub monthly_cost_nusd: i64,
    pub uncertain: i64,
    pub reserved: i64,
    pub newest_embedded_at: Option<i64>,
    pub oldest_pending_at: Option<i64>,
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
           SELECT event.id,reader.received_at
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
               WHERE post.event_id=eligible.id AND post.space_id=space.id)
               AND NOT EXISTS (SELECT 1 FROM embedding_input_rejections rejection
                 WHERE rejection.event_id=eligible.id AND rejection.space_id=space.id)),
           (SELECT count(*) FROM embedding_input_rejections rejection
             JOIN eligible ON eligible.id=rejection.event_id WHERE rejection.space_id=space.id),
           (SELECT count(*) FROM embedding_requests request
             WHERE request.space_id=space.id AND request.status='succeeded'),
           (SELECT coalesce(sum(request.actual_tokens),0) FROM embedding_requests request
             WHERE request.space_id=space.id AND request.status='succeeded'),
           (SELECT coalesce(sum(request.actual_nusd),0) FROM embedding_requests request
             WHERE request.space_id=space.id AND request.status='succeeded'),
           (SELECT coalesce(sum(request.actual_nusd),0) FROM embedding_requests request
             WHERE request.space_id=space.id AND request.status='succeeded'
               AND request.requested_at>=unixepoch(?2,'unixepoch','start of month')),
           (SELECT count(*) FROM embedding_requests request
             WHERE request.space_id=space.id AND request.status='uncertain'),
           (SELECT count(*) FROM embedding_requests request
             WHERE request.space_id=space.id AND request.status='reserved'),
           (SELECT max(post.embedded_at) FROM post_embeddings post
             JOIN eligible ON eligible.id=post.event_id WHERE post.space_id=space.id),
           (SELECT min(eligible.received_at) FROM eligible
             WHERE NOT EXISTS (SELECT 1 FROM post_embeddings post
               WHERE post.event_id=eligible.id AND post.space_id=space.id))
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
                rejected: row.get(9)?,
                requests: row.get(10)?,
                tokens: row.get(11)?,
                cost_nusd: row.get(12)?,
                monthly_cost_nusd: row.get(13)?,
                uncertain: row.get(14)?,
                reserved: row.get(15)?,
                newest_embedded_at: row.get(16)?,
                oldest_pending_at: row.get(17)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(statuses)
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct TopicSettings {
    pub kmeans_k: usize,
    pub dbscan_epsilon_cosine: f32,
    pub dbscan_min_samples: usize,
}

impl TopicSettings {
    pub fn from_env() -> Result<Self, Error> {
        let kmeans_k = std::env::var("MEATYBROTH_KMEANS_K")
            .unwrap_or_else(|_| "24".into())
            .parse()?;
        let dbscan_epsilon_cosine = std::env::var("MEATYBROTH_DBSCAN_EPSILON_COSINE")
            .unwrap_or_else(|_| "0.20".into())
            .parse()?;
        let dbscan_min_samples = std::env::var("MEATYBROTH_DBSCAN_MIN_SAMPLES")
            .unwrap_or_else(|_| "8".into())
            .parse()?;
        if kmeans_k == 0 || dbscan_min_samples < 2 {
            return Err(
                "Topic k must be positive and DBSCAN min_samples must be at least 2".into(),
            );
        }
        if !(0.0 < dbscan_epsilon_cosine && dbscan_epsilon_cosine <= 2.0) {
            return Err(
                "DBSCAN cosine-distance epsilon must be greater than 0 and at most 2".into(),
            );
        }
        Ok(Self {
            kmeans_k,
            dbscan_epsilon_cosine,
            dbscan_min_samples,
        })
    }
}

pub fn stored_topic_settings(path: &Path, space: &Space) -> Result<Option<TopicSettings>, Error> {
    let conn = db(path)?;
    let kmeans_k: Option<usize> = conn
        .query_row(
            "SELECT kmeans_k FROM embedding_topic_builds WHERE space_id=?1 AND method='kmeans'",
            [&space.id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten()
        .map(usize::try_from)
        .transpose()?;
    let legacy_kmeans_k: usize = conn
        .query_row(
            "SELECT count(*) FROM embedding_topics WHERE space_id=?1",
            [&space.id],
            |row| row.get::<_, i64>(0),
        )?
        .try_into()?;
    let dbscan: Option<(f32, usize)> = conn.query_row(
        "SELECT epsilon_cosine,min_samples FROM embedding_topic_builds WHERE space_id=?1 AND method='dbscan'",
        [&space.id],
        |row| Ok((row.get::<_, f32>(0)?, usize::try_from(row.get::<_, i64>(1)?).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?)),
    ).optional()?;
    let legacy_dbscan: Option<(f32, usize)> = conn.query_row(
        "SELECT epsilon_cosine,min_samples FROM embedding_dbscan_topics WHERE space_id=?1 LIMIT 1",
        [&space.id],
        |row| Ok((row.get::<_, f32>(0)?, usize::try_from(row.get::<_, i64>(1)?).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?)),
    ).optional()?;
    match (
        kmeans_k.or((legacy_kmeans_k > 0).then_some(legacy_kmeans_k)),
        dbscan.or(legacy_dbscan),
    ) {
        (Some(kmeans_k), Some((dbscan_epsilon_cosine, dbscan_min_samples))) => {
            Ok(Some(TopicSettings {
                kmeans_k,
                dbscan_epsilon_cosine,
                dbscan_min_samples,
            }))
        }
        _ => Ok(None),
    }
}

/// One keyword-labelled cluster in an exact embedding space.
#[derive(Debug, serde::Serialize)]
pub struct Topic {
    pub id: i64,
    pub label: String,
    pub post_count: i64,
    pub percent: String,
}

fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
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

fn document_terms(text: &str) -> std::collections::HashSet<String> {
    // stopwords-iso English list, MIT, pinned at ccc8898. -- Pi/gpt-5.6-sol
    // https://github.com/stopwords-iso/stopwords-en/tree/ccc8898188850d8fb019d5f69c14a6635c3bd115
    static STOP: LazyLock<std::collections::HashSet<&'static str>> =
        LazyLock::new(|| include_str!("data/english-stopwords.txt").lines().collect());
    static URLS: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?i)(?:https?://|www\.)\S+").unwrap());
    static WORDS: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?i)[\p{L}\p{N}][\p{L}\p{N}_-]{2,}").unwrap());
    WORDS
        .find_iter(&URLS.replace_all(text, " "))
        .map(|word| word.as_str().to_lowercase())
        .filter(|word| {
            word.chars().count() <= 40
                && (word.is_ascii() || word.chars().count() <= 16)
                && word.chars().any(char::is_alphabetic)
                && !(word.is_ascii() && STOP.contains(word.as_str()))
        })
        .collect()
}

fn document_frequencies(
    documents: &[std::collections::HashSet<String>],
) -> std::collections::HashMap<String, usize> {
    let mut frequencies = std::collections::HashMap::new();
    for document in documents {
        for term in document {
            *frequencies.entry(term.clone()).or_default() += 1;
        }
    }
    frequencies
}

fn script_fallback(frequencies: &std::collections::HashMap<String, usize>) -> Option<&'static str> {
    let mut alphabetic = 0_usize;
    let mut japanese = 0_usize;
    let mut arabic = 0_usize;
    for (term, frequency) in frequencies {
        for character in term.chars().filter(|character| character.is_alphabetic()) {
            alphabetic += frequency;
            let code = character as u32;
            if (0x3040..=0x30ff).contains(&code) || (0x4e00..=0x9fff).contains(&code) {
                japanese += frequency;
            }
            if (0x0600..=0x06ff).contains(&code) {
                arabic += frequency;
            }
        }
    }
    (alphabetic > 0 && japanese * 5 >= alphabetic * 3)
        .then_some("Japanese-script text")
        .or_else(|| {
            (alphabetic > 0 && arabic * 5 >= alphabetic * 3).then_some("Arabic-script text")
        })
}

fn topic_label(
    member_indices: &[usize],
    representative_indices: &[usize],
    documents: &[std::collections::HashSet<String>],
    corpus_frequencies: &std::collections::HashMap<String, usize>,
) -> String {
    let mut cluster_frequencies = std::collections::HashMap::<String, usize>::new();
    for index in member_indices {
        for term in &documents[*index] {
            *cluster_frequencies.entry(term.clone()).or_default() += 1;
        }
    }
    if let Some(label) = script_fallback(&cluster_frequencies) {
        return label.into();
    }
    let minimum_support = 3.max(member_indices.len().div_ceil(5));
    let minimum_representatives =
        (representative_indices.len().div_ceil(5) * 3).min(representative_indices.len());
    let member_document_count = member_indices.len();
    let other_document_count = documents.len().saturating_sub(member_document_count);
    let member_count = member_document_count as f64;
    let other_count = other_document_count as f64;
    let mut terms = cluster_frequencies
        .into_iter()
        .filter_map(|(term, cluster_frequency)| {
            let other_frequency = corpus_frequencies[&term].saturating_sub(cluster_frequency);
            // Each document contributes at most once. Beta(1,1) smoothing makes the
            // transparent unique-tail score log p(term|topic)-log p(term|not-topic).
            let topic_probability = (cluster_frequency as f64 + 1.0) / (member_count + 2.0);
            let other_probability = (other_frequency as f64 + 1.0) / (other_count + 2.0);
            let unique_tail = (topic_probability / other_probability).ln();
            let representative_frequency = representative_indices
                .iter()
                .filter(|index| documents[**index].contains(&term))
                .count();
            (cluster_frequency >= minimum_support
                // Do not let smoothing turn a term present in every document into a
                // false topic tail merely because the two populations have different sizes.
                && cluster_frequency * other_document_count
                    > other_frequency * member_document_count
                && unique_tail > 0.0
                && representative_frequency >= minimum_representatives)
                .then_some((term, cluster_frequency, unique_tail))
        })
        .collect::<Vec<_>>();
    terms.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.0.cmp(&right.0))
    });
    let label = terms
        .into_iter()
        .take(3)
        .map(|(term, _, _)| term)
        .collect::<Vec<_>>()
        .join(" · ");
    if label.is_empty() {
        "Unlabelled topic".into()
    } else {
        label
    }
}

type ClusterInput = (Vec<u8>, Vec<f32>, String);

fn cluster_input(conn: &Connection, space: &Space, now: i64) -> Result<Vec<ClusterInput>, Error> {
    let mut statement = conn.prepare(
        "SELECT embedding.event_id,embedding.vector,event.content
         FROM post_embeddings embedding
         JOIN events event ON event.id=embedding.event_id
         JOIN reader_post_events reader ON reader.event_id=event.id
         WHERE embedding.space_id=?1 AND event.created_at BETWEEN ?2 AND ?3
         ORDER BY embedding.event_id",
    )?;
    let rows = statement
        .query_map((&space.id, now - WINDOW, now), |row| {
            Ok((
                row.get(0)?,
                decode_vector(&row.get::<_, Vec<u8>>(1)?, space.dimensions)
                    .map_err(rusqlite::Error::ToSqlConversionFailure)?,
                row.get(2)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

fn current_topic_event_ids(
    conn: &Connection,
    space: &Space,
    now: i64,
) -> Result<std::collections::HashSet<Vec<u8>>, Error> {
    let mut statement = conn.prepare(
        "SELECT embedding.event_id FROM post_embeddings embedding
         JOIN events event ON event.id=embedding.event_id
         JOIN reader_post_events reader ON reader.event_id=event.id
         WHERE embedding.space_id=?1 AND event.created_at BETWEEN ?2 AND ?3",
    )?;
    let event_ids = statement
        .query_map((&space.id, now - WINDOW, now), |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(event_ids)
}

/// Recomputes deterministic cosine clusters and labels from current post text.
pub fn cluster_topics(path: &Path, space: &Space, now: i64) -> Result<usize, Error> {
    if !space.normalize {
        return Err("Cosine topic clustering requires normalized vectors".into());
    }
    let mut conn = db(path)?;
    let rows = cluster_input(&conn, space, now)?;
    if rows.is_empty() {
        let settings = TopicSettings::from_env()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM embedding_topics WHERE space_id=?1",
            [&space.id],
        )?;
        tx.execute(
            "INSERT INTO embedding_topic_builds(space_id,method,kmeans_k,epsilon_cosine,min_samples,built_at)
             VALUES(?1,'kmeans',?2,NULL,NULL,?3)
             ON CONFLICT(space_id,method) DO UPDATE SET kmeans_k=excluded.kmeans_k,
               epsilon_cosine=NULL,min_samples=NULL,built_at=excluded.built_at",
            rusqlite::params![space.id, i64::try_from(settings.kmeans_k)?, now],
        )?;
        tx.commit()?;
        return Ok(0);
    }
    let cluster_count = TopicSettings::from_env()?.kmeans_k.min(rows.len());
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
    let documents = rows
        .iter()
        .map(|(_, _, text)| document_terms(text))
        .collect::<Vec<_>>();
    let corpus_frequencies = document_frequencies(&documents);
    let labels: Vec<_> = (0..cluster_count)
        .map(|cluster| {
            let member_indices = assignments
                .iter()
                .enumerate()
                .filter_map(|(index, assignment)| (*assignment == cluster).then_some(index))
                .collect::<Vec<_>>();
            let mut representative_indices = member_indices.clone();
            representative_indices.sort_by(|left, right| {
                dot(&rows[*right].1, &centroids[cluster])
                    .total_cmp(&dot(&rows[*left].1, &centroids[cluster]))
            });
            representative_indices.truncate(5);
            (
                topic_label(
                    &member_indices,
                    &representative_indices,
                    &documents,
                    &corpus_frequencies,
                ),
                representative_indices
                    .iter()
                    .map(|index| rows[*index].0.clone())
                    .collect::<Vec<Vec<u8>>>(),
            )
        })
        .collect();
    let mut centroid_sums = vec![vec![0.0_f32; space.dimensions]; cluster_count];
    for ((_, vector, _), assignment) in rows.iter().zip(&assignments) {
        for (sum, value) in centroid_sums[*assignment].iter_mut().zip(vector) {
            *sum += value;
        }
    }
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute(
        "DELETE FROM embedding_topics WHERE space_id=?1",
        [&space.id],
    )?;
    let current_events = current_topic_event_ids(&tx, space, now)?;
    let mut stored_topics = 0;
    for (topic_id, (label, representative_ids)) in labels.iter().enumerate() {
        let post_count = rows
            .iter()
            .zip(&assignments)
            .filter(|((event_id, _, _), assignment)| {
                **assignment == topic_id && current_events.contains(event_id)
            })
            .count();
        if post_count == 0 {
            continue;
        }
        let label = if representative_ids
            .iter()
            .any(|event_id| !current_events.contains(event_id))
        {
            "Unlabelled topic"
        } else {
            label
        };
        let mut centroid = centroid_sums[topic_id].clone();
        for ((event_id, vector, _), assignment) in rows.iter().zip(&assignments) {
            if *assignment == topic_id && !current_events.contains(event_id) {
                for (sum, value) in centroid.iter_mut().zip(vector) {
                    *sum -= value;
                }
            }
        }
        normalize(&mut centroid)?;
        tx.execute(
            "INSERT INTO embedding_topics(space_id,topic_id,label,post_count,centroid,created_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            (
                &space.id,
                i64::try_from(topic_id)?,
                label,
                i64::try_from(post_count)?,
                vector_bytes(&centroid),
                now,
            ),
        )?;
        stored_topics += 1;
    }
    for ((event_id, _, _), topic_id) in rows.iter().zip(assignments) {
        if current_events.contains(event_id) {
            tx.execute(
                "INSERT INTO post_topics(event_id,space_id,topic_id) VALUES(?1,?2,?3)",
                (event_id, &space.id, i64::try_from(topic_id)?),
            )?;
        }
    }
    tx.execute(
        "INSERT INTO embedding_topic_builds(space_id,method,kmeans_k,epsilon_cosine,min_samples,built_at)
         VALUES(?1,'kmeans',?2,NULL,NULL,?3)
         ON CONFLICT(space_id,method) DO UPDATE SET kmeans_k=excluded.kmeans_k,
           epsilon_cosine=NULL,min_samples=NULL,built_at=excluded.built_at",
        rusqlite::params![space.id, i64::try_from(TopicSettings::from_env()?.kmeans_k)?, now],
    )?;
    tx.commit()?;
    Ok(stored_topics)
}

/// Recomputes exact DBSCAN clusters over normalized vectors. -- Pi/gpt-5.6-sol
pub fn cluster_dbscan_topics(
    path: &Path,
    space: &Space,
    now: i64,
    epsilon_cosine: f32,
    min_samples: usize,
) -> Result<usize, Error> {
    if !space.normalize {
        return Err("DBSCAN cosine distance requires normalized vectors".into());
    }
    if min_samples < 2 || !(0.0 < epsilon_cosine && epsilon_cosine <= 2.0) {
        return Err("DBSCAN requires min_samples >= 2 and cosine epsilon in (0,2]".into());
    }
    let mut conn = db(path)?;
    let rows = cluster_input(&conn, space, now)?;
    if rows.is_empty() {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM embedding_dbscan_topics WHERE space_id=?1",
            [&space.id],
        )?;
        tx.execute(
            "INSERT INTO embedding_topic_builds(space_id,method,kmeans_k,epsilon_cosine,min_samples,built_at)
             VALUES(?1,'dbscan',NULL,?2,?3,?4)
             ON CONFLICT(space_id,method) DO UPDATE SET kmeans_k=NULL,
               epsilon_cosine=excluded.epsilon_cosine,min_samples=excluded.min_samples,built_at=excluded.built_at",
            rusqlite::params![space.id, epsilon_cosine, i64::try_from(min_samples)?, now],
        )?;
        tx.commit()?;
        return Ok(0);
    }
    let matrix = Array2::from_shape_vec(
        (rows.len(), space.dimensions),
        rows.iter()
            .flat_map(|(_, vector, _)| vector.iter().copied())
            .collect(),
    )?;
    let epsilon_l2 = (2.0 * epsilon_cosine).sqrt();
    let labels = if min_samples > rows.len() {
        vec![None; rows.len()]
    } else {
        Dbscan::params_with(min_samples, L2Dist, BallTree)
            .tolerance(epsilon_l2)
            .transform(&matrix)
            .map_err(|error| std::io::Error::other(error.to_string()))?
            .to_vec()
    };
    let index = BallTree
        .from_batch(&matrix, L2Dist)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let core: Vec<bool> = labels
        .iter()
        .zip(matrix.outer_iter())
        .map(|(label, point): (&Option<usize>, ArrayView1<'_, f32>)| {
            if label.is_none() {
                return Ok(false);
            }
            index
                .k_nearest(point, min_samples)
                .map(|neighbors| L2Dist.distance(point, neighbors.last().unwrap().0) <= epsilon_l2)
                .map_err(|error| std::io::Error::other(error.to_string()))
        })
        .collect::<Result<_, _>>()?;
    let topic_ids: Vec<i64> = labels
        .iter()
        .map(|label| label.map_or(-1, |topic| i64::try_from(topic).unwrap()))
        .collect();
    let mut members = std::collections::BTreeMap::<i64, Vec<usize>>::new();
    for (index, topic_id) in topic_ids.iter().copied().enumerate() {
        members.entry(topic_id).or_default().push(index);
    }
    members.entry(-1).or_default();
    let documents = rows
        .iter()
        .map(|(_, _, text)| document_terms(text))
        .collect::<Vec<_>>();
    let corpus_frequencies = document_frequencies(&documents);
    let mut prepared_topics = Vec::with_capacity(members.len());
    for (topic_id, indices) in &members {
        let centroid_sum = if *topic_id == -1 {
            None
        } else {
            let mut vector = vec![0.0; space.dimensions];
            for index in indices {
                for (total, value) in vector.iter_mut().zip(&rows[*index].1) {
                    *total += value;
                }
            }
            Some(vector)
        };
        let centroid_vector = centroid_sum
            .as_ref()
            .map(|sum| {
                let mut centroid = sum.clone();
                normalize(&mut centroid).map(|()| centroid)
            })
            .transpose()?;
        let (label, representative_ids) = if let Some(centroid) = &centroid_vector {
            let mut representatives = indices
                .iter()
                .map(|index| (dot(&rows[*index].1, centroid), *index))
                .collect::<Vec<_>>();
            representatives.sort_by(|left, right| right.0.total_cmp(&left.0));
            let representative_indices = representatives
                .into_iter()
                .take(5)
                .map(|(_, index)| index)
                .collect::<Vec<_>>();
            (
                topic_label(
                    indices,
                    &representative_indices,
                    &documents,
                    &corpus_frequencies,
                ),
                representative_indices
                    .iter()
                    .map(|index| rows[*index].0.clone())
                    .collect(),
            )
        } else {
            ("Noise / unmatched".into(), Vec::new())
        };
        prepared_topics.push((*topic_id, label, representative_ids, centroid_sum));
    }
    let transaction = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    transaction.execute(
        "DELETE FROM post_dbscan_topics WHERE space_id=?1",
        [&space.id],
    )?;
    transaction.execute(
        "DELETE FROM embedding_dbscan_topics WHERE space_id=?1",
        [&space.id],
    )?;
    let current_events = current_topic_event_ids(&transaction, space, now)?;
    let mut stored_topics = 0;
    for (topic_id, mut label, representative_ids, centroid_sum) in prepared_topics {
        let current_indices = members[&topic_id]
            .iter()
            .filter(|index| current_events.contains(&rows[**index].0))
            .collect::<Vec<_>>();
        if current_indices.is_empty() {
            continue;
        }
        if representative_ids
            .iter()
            .any(|event_id| !current_events.contains(event_id))
        {
            label = "Unlabelled topic".into();
        }
        let centroid = if let Some(mut centroid) = centroid_sum {
            for index in &members[&topic_id] {
                if !current_events.contains(&rows[*index].0) {
                    for (sum, value) in centroid.iter_mut().zip(&rows[*index].1) {
                        *sum -= value;
                    }
                }
            }
            normalize(&mut centroid)?;
            Some(vector_bytes(&centroid))
        } else {
            None
        };
        transaction.execute(
            "INSERT INTO embedding_dbscan_topics
             (space_id,topic_id,label,post_count,epsilon_cosine,min_samples,centroid,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![
                space.id,
                topic_id,
                label,
                i64::try_from(current_indices.len())?,
                epsilon_cosine,
                i64::try_from(min_samples)?,
                centroid,
                now
            ],
        )?;
        stored_topics += 1;
        for index in current_indices {
            transaction.execute(
                "INSERT INTO post_dbscan_topics(event_id,space_id,topic_id,is_core,assigned_at)
                 VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![rows[*index].0, space.id, topic_id, core[*index], now],
            )?;
        }
    }
    transaction.execute(
        "INSERT INTO embedding_topic_builds(space_id,method,kmeans_k,epsilon_cosine,min_samples,built_at)
         VALUES(?1,'dbscan',NULL,?2,?3,?4)
         ON CONFLICT(space_id,method) DO UPDATE SET kmeans_k=NULL,
           epsilon_cosine=excluded.epsilon_cosine,min_samples=excluded.min_samples,built_at=excluded.built_at",
        rusqlite::params![space.id, epsilon_cosine, i64::try_from(min_samples)?, now],
    )?;
    transaction.commit()?;
    Ok(stored_topics)
}

pub fn rebuild_topics(path: &Path, space: &Space, now: i64) -> Result<(usize, usize), Error> {
    let settings = TopicSettings::from_env()?;
    let kmeans = cluster_topics(path, space, now)?;
    let dbscan = cluster_dbscan_topics(
        path,
        space,
        now,
        settings.dbscan_epsilon_cosine,
        settings.dbscan_min_samples,
    )?;
    Ok((kmeans, dbscan))
}

pub fn topics_due(path: &Path, space: &Space, now: i64) -> Result<bool, Error> {
    let settings = TopicSettings::from_env()?;
    let conn = db(path)?;
    let matching_kmeans: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM embedding_topic_builds
          WHERE space_id=?1 AND method='kmeans' AND kmeans_k=?2)",
        rusqlite::params![space.id, i64::try_from(settings.kmeans_k)?],
        |row| row.get(0),
    )?;
    let matching_dbscan: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM embedding_topic_builds
          WHERE space_id=?1 AND method='dbscan' AND epsilon_cosine=?2 AND min_samples=?3)",
        rusqlite::params![
            space.id,
            settings.dbscan_epsilon_cosine,
            i64::try_from(settings.dbscan_min_samples)?
        ],
        |row| row.get(0),
    )?;
    if !matching_kmeans || !matching_dbscan {
        return Ok(true);
    }
    let obsolete_label: bool = conn.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM embedding_topics WHERE space_id=?1 AND label='mixed'
           UNION ALL
           SELECT 1 FROM embedding_dbscan_topics WHERE space_id=?1 AND label='mixed'
         )",
        [&space.id],
        |row| row.get(0),
    )?;
    if obsolete_label {
        return Ok(true);
    }
    let kmeans_at: Option<i64> = conn.query_row(
        "SELECT max(created_at) FROM embedding_topics WHERE space_id=?1",
        [&space.id],
        |row| row.get(0),
    )?;
    let dbscan_at: Option<i64> = conn.query_row(
        "SELECT max(created_at) FROM embedding_dbscan_topics WHERE space_id=?1",
        [&space.id],
        |row| row.get(0),
    )?;
    Ok(kmeans_at
        .min(dbscan_at)
        .is_none_or(|built| now - built >= 6 * 3600))
}

/// Assigns new vectors without promoting DBSCAN cores or merging clusters. -- Pi/gpt-5.6-sol
pub fn assign_new_topics(path: &Path, space: &Space, now: i64) -> Result<usize, Error> {
    let settings = TopicSettings::from_env()?;
    let mut conn = db(path)?;
    let kmeans: Vec<(i64, Vec<f32>)> = {
        let mut statement = conn.prepare(
            "SELECT topic_id,centroid FROM embedding_topics WHERE space_id=?1 ORDER BY topic_id",
        )?;
        let rows = statement
            .query_map([&space.id], |row| {
                Ok((
                    row.get(0)?,
                    decode_vector(&row.get::<_, Vec<u8>>(1)?, space.dimensions)
                        .map_err(rusqlite::Error::ToSqlConversionFailure)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        rows
    };
    let dbscan_cores: Vec<(i64, Vec<f32>)> = {
        let mut statement = conn.prepare(
            "SELECT membership.topic_id,embedding.vector
             FROM post_dbscan_topics membership
             JOIN post_embeddings embedding ON embedding.event_id=membership.event_id
               AND embedding.space_id=membership.space_id
             WHERE membership.space_id=?1 AND membership.is_core=1",
        )?;
        let rows = statement
            .query_map([&space.id], |row| {
                Ok((
                    row.get(0)?,
                    decode_vector(&row.get::<_, Vec<u8>>(1)?, space.dimensions)
                        .map_err(rusqlite::Error::ToSqlConversionFailure)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        rows
    };
    let dbscan_exists = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM embedding_dbscan_topics WHERE space_id=?1)",
        [&space.id],
        |row| row.get::<_, bool>(0),
    )?;
    let pending: Vec<(Vec<u8>, Vec<f32>, bool, bool)> = {
        let mut statement = conn.prepare(
            "SELECT embedding.event_id,embedding.vector,
               EXISTS(SELECT 1 FROM post_topics topic WHERE topic.event_id=embedding.event_id AND topic.space_id=embedding.space_id),
               EXISTS(SELECT 1 FROM post_dbscan_topics topic WHERE topic.event_id=embedding.event_id AND topic.space_id=embedding.space_id)
             FROM post_embeddings embedding
             JOIN reader_post_events reader ON reader.event_id=embedding.event_id
             WHERE embedding.space_id=?1 AND (
               NOT EXISTS(SELECT 1 FROM post_topics topic WHERE topic.event_id=embedding.event_id AND topic.space_id=embedding.space_id)
               OR NOT EXISTS(SELECT 1 FROM post_dbscan_topics topic WHERE topic.event_id=embedding.event_id AND topic.space_id=embedding.space_id))",
        )?;
        let rows = statement
            .query_map([&space.id], |row| {
                Ok((
                    row.get(0)?,
                    decode_vector(&row.get::<_, Vec<u8>>(1)?, space.dimensions)
                        .map_err(rusqlite::Error::ToSqlConversionFailure)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        rows
    };
    let transaction = conn.transaction()?;
    for (event_id, vector, has_kmeans, has_dbscan) in &pending {
        if !has_kmeans && !kmeans.is_empty() {
            let topic_id = kmeans
                .iter()
                .max_by(|(_, left), (_, right)| dot(vector, left).total_cmp(&dot(vector, right)))
                .unwrap()
                .0;
            transaction.execute(
                "INSERT INTO post_topics(event_id,space_id,topic_id) VALUES(?1,?2,?3)",
                rusqlite::params![event_id, space.id, topic_id],
            )?;
            transaction.execute(
                "UPDATE embedding_topics SET post_count=post_count+1 WHERE space_id=?1 AND topic_id=?2",
                rusqlite::params![space.id, topic_id],
            )?;
        }
        if !has_dbscan && dbscan_exists {
            let nearest = dbscan_cores
                .iter()
                .map(|(topic_id, core)| (*topic_id, 1.0 - dot(vector, core)))
                .min_by(|(_, left), (_, right)| left.total_cmp(right));
            let topic_id = nearest
                .filter(|(_, distance)| *distance <= settings.dbscan_epsilon_cosine)
                .map_or(-1, |(topic_id, _)| topic_id);
            transaction.execute(
                "INSERT INTO post_dbscan_topics(event_id,space_id,topic_id,is_core,assigned_at)
                 VALUES(?1,?2,?3,0,?4)",
                rusqlite::params![event_id, space.id, topic_id, now],
            )?;
            transaction.execute(
                "UPDATE embedding_dbscan_topics SET post_count=post_count+1 WHERE space_id=?1 AND topic_id=?2",
                rusqlite::params![space.id, topic_id],
            )?;
        }
    }
    transaction.commit()?;
    Ok(pending.len())
}

/// Lists keyword-labelled topics largest first. -- Pi/gpt-5.6-sol
pub fn topics(path: &Path, space: &Space, algorithm: &str) -> Result<Vec<Topic>, Error> {
    let conn = db(path)?;
    let (topic_table, membership_table) = match algorithm {
        "kmeans" => ("embedding_topics", "post_topics"),
        "dbscan" => ("embedding_dbscan_topics", "post_dbscan_topics"),
        _ => return Err(format!("Unknown topic algorithm: {algorithm}").into()),
    };
    let sql = format!(
        "SELECT topic.topic_id,topic.label,count(member.event_id)
         FROM {topic_table} topic LEFT JOIN {membership_table} member
           ON member.space_id=topic.space_id AND member.topic_id=topic.topic_id
         WHERE topic.space_id=?1
         GROUP BY topic.topic_id,topic.label
         ORDER BY count(member.event_id) DESC,topic.topic_id"
    );
    let mut statement = conn.prepare(&sql)?;
    let mut topics = statement
        .query_map([&space.id], |row| {
            Ok(Topic {
                id: row.get(0)?,
                label: row.get(1)?,
                post_count: row.get(2)?,
                percent: String::new(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let population = topics.iter().map(|topic| topic.post_count).sum::<i64>();
    for topic in &mut topics {
        topic.percent = if population == 0 {
            "0.0".into()
        } else {
            format!("{:.1}", 100.0 * topic.post_count as f64 / population as f64)
        };
    }
    Ok(topics)
}

pub struct TopicPage<'a> {
    pub ids: &'a [i64],
    pub include_unsorted: bool,
    pub algorithm: &'a str,
    pub now: i64,
    pub limit: usize,
    pub offset: usize,
}

/// Returns selected topic event IDs in newest-first order. -- Pi/gpt-5.6-sol
pub fn topic_events(
    path: &Path,
    space: &Space,
    page: TopicPage<'_>,
) -> Result<Vec<Vec<u8>>, Error> {
    let conn = db(path)?;
    let table = match page.algorithm {
        "kmeans" => "post_topics",
        "dbscan" => "post_dbscan_topics",
        _ => return Err(format!("Unknown topic algorithm: {}", page.algorithm).into()),
    };
    let sql = format!(
        "SELECT topic.event_id FROM events event INDEXED BY idx_events_created_at
         JOIN {table} topic ON topic.event_id=event.id
         JOIN reader_post_events reader ON reader.event_id=event.id
         WHERE topic.space_id=?1
           AND ((?2='[]' AND NOT ?3 AND topic.topic_id!=-1)
             OR topic.topic_id IN (SELECT value FROM json_each(?2))
             OR (?3 AND topic.topic_id=-1))
           AND event.created_at BETWEEN ?4 AND ?5
         ORDER BY event.created_at DESC,event.id LIMIT ?6 OFFSET ?7"
    );
    let mut statement = conn.prepare(&sql)?;
    let topic_ids = serde_json::to_string(page.ids)?;
    let events = statement
        .query_map(
            (
                &space.id,
                topic_ids,
                page.include_unsorted,
                page.now - WINDOW,
                page.now,
                i64::try_from(page.limit)?,
                i64::try_from(page.offset)?,
            ),
            |row| row.get(0),
        )?
        .collect::<Result<_, _>>()
        .map_err(Into::into);
    events
}

/// Loads one event vector from the model's exact space.
pub fn vector_event_ids(
    path: &Path,
    space: &Space,
    event_ids: &[Vec<u8>],
) -> Result<std::collections::HashSet<Vec<u8>>, Error> {
    if event_ids.is_empty() {
        return Ok(Default::default());
    }
    let placeholders = (1..=event_ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT event_id FROM post_embeddings WHERE space_id=?{} AND event_id IN ({placeholders})",
        event_ids.len() + 1
    );
    let mut values: Vec<&dyn rusqlite::ToSql> = event_ids
        .iter()
        .map(|event_id| event_id as &dyn rusqlite::ToSql)
        .collect();
    values.push(&space.id);
    let conn = db(path)?;
    let mut statement = conn.prepare(&sql)?;
    let found = statement
        .query_map(values.as_slice(), |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(found)
}

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
    use super::{document_frequencies, document_terms, topic_label};

    fn label(texts: &[&str], members: &[usize], representatives: &[usize]) -> String {
        let documents = texts
            .iter()
            .map(|text| document_terms(text))
            .collect::<Vec<_>>();
        topic_label(
            members,
            representatives,
            &documents,
            &document_frequencies(&documents),
        )
    }

    #[test]
    fn labels_use_contrastive_unicode_terms_from_members_and_check_representatives() {
        let texts = [
            "good year all",
            "them because don't",
            "rust vector search",
            "rust vector index",
            "rust vector",
        ];
        assert_eq!(label(&texts, &[0, 1], &[0, 1]), "Unlabelled topic");
        assert_eq!(label(&texts, &[2, 3, 4], &[2, 3, 4]), "rust · vector");
        assert_eq!(
            label(
                &[
                    "market privacy wallet",
                    "market privacy protocol",
                    "market privacy lightning",
                    "market garden fruit",
                    "market birds trees",
                    "market ocean waves",
                ],
                &[0, 1, 2],
                &[0, 1, 2],
            ),
            "privacy"
        );
        assert_eq!(
            label(
                &[
                    "rust protocol",
                    "rust implementation",
                    "ordinary note",
                    "garden soil",
                    "orchard fruit",
                    "birds flying",
                ],
                &[0, 1, 2],
                &[0, 2],
            ),
            "Unlabelled topic"
        );
        assert_eq!(
            label(
                &[
                    "東京経済 ニュース",
                    "東京経済 市場",
                    "rust code",
                    "garden soil"
                ],
                &[0, 1],
                &[0]
            ),
            "Japanese-script text"
        );
        assert_eq!(
            label(
                &[
                    "plain words https://same.example/path",
                    "quantum https://same.example/path",
                    "quantum https://same.example/path",
                    "garden soil",
                    "orchard fruit",
                    "birds flying",
                ],
                &[0, 1, 2],
                &[0]
            ),
            "Unlabelled topic"
        );
    }
}
