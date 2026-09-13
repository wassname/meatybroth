//! Prepares Titan V2 embeddings for admitted notes in the reader's SQLite file.
//!
//! Request rows are the durable cost ledger; chunk and post vectors belong to SDK events.
//! -- Pi/gpt-5.6-sol

use crate::{queries::WINDOW, Error};
use rusqlite::{Connection, OptionalExtension};
use std::{future::Future, path::Path, pin::Pin};

pub const MODEL: &str = "amazon.titan-embed-text-v2:0";
pub const DIMENSIONS: usize = 512;
pub const NORMALIZE: bool = true;
const DIMENSIONS_SQL: i64 = 512;
const MAX_CHUNK_BYTES: usize = 8_000;
const PRICE_NUSD_PER_TOKEN: i64 = 20;

/// Hard limits checked against reserved and completed request cost.
#[derive(Clone, Copy)]
pub struct Budget {
    pub total_nusd: i64,
    pub monthly_nusd: i64,
}

/// One provider request with fixed model settings.
pub struct Input<'a> {
    pub text: &'a str,
    pub model: &'static str,
    pub dimensions: usize,
    pub normalize: bool,
}

/// Provider output and its billed input-token count.
pub struct Output {
    pub vector: Vec<f32>,
    pub input_tokens: i64,
}

/// Transport boundary used by Bedrock in production and a deterministic mock in tests.
pub trait Transport: Send + Sync {
    fn embed<'a>(
        &'a self,
        input: Input<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Output, Error>> + Send + 'a>>;
}

fn db(path: &Path) -> Result<Connection, Error> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    Ok(conn)
}

/// Splits UTF-8 text without changing its bytes or exceeding Titan's input bound.
pub fn chunks(text: &str) -> Vec<&str> {
    assert!(!text.trim().is_empty());
    let mut chunks = Vec::new();
    let mut start = 0;
    for (index, character) in text.char_indices() {
        if index + character.len_utf8() - start > MAX_CHUNK_BYTES {
            chunks.push(&text[start..index]);
            start = index;
        }
    }
    chunks.push(&text[start..]);
    chunks
}

fn pending(path: &Path, now: i64, limit: usize) -> Result<Vec<(Vec<u8>, String)>, Error> {
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
             SELECT 1 FROM post_embeddings v
             WHERE v.event_id=e.id AND v.model=?3 AND v.dimensions=?4 AND v.normalize=1)
         ORDER BY e.id LIMIT ?5",
    )?;
    let rows = statement.query_map(
        (
            now - WINDOW,
            now,
            MODEL,
            DIMENSIONS_SQL,
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
) -> Result<Option<String>, Error> {
    conn.query_row(
        "SELECT status FROM embedding_requests
         WHERE event_id=?1 AND chunk_index=?2 AND model=?3 AND dimensions=?4 AND normalize=1",
        (event_id, i64::try_from(chunk_index)?, MODEL, DIMENSIONS_SQL),
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
    budget: Budget,
    now: i64,
) -> Result<Option<i64>, Error> {
    let mut conn = db(path)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if let Some(status) = request_status(&tx, event_id, chunk_index)? {
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
    let reserved = i64::try_from(input_bytes)? * PRICE_NUSD_PER_TOKEN;
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
           event_id,chunk_index,model,dimensions,normalize,requested_at,reserved_nusd,status)
         VALUES(?1,?2,?3,?4,1,?5,?6,'reserved')",
        (
            event_id,
            i64::try_from(chunk_index)?,
            MODEL,
            DIMENSIONS_SQL,
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

fn validate(output: &Output) -> Result<(), Error> {
    if output.vector.len() != DIMENSIONS
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

fn complete_request(path: &Path, completion: Completion<'_>) -> Result<(), Error> {
    let Completion {
        request_id,
        event_id,
        chunk_index,
        chunk_count,
        input_bytes,
        output,
        now,
    } = completion;
    validate(output)?;
    let actual_nusd = output.input_tokens * PRICE_NUSD_PER_TOKEN;
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
           event_id,chunk_index,chunk_count,model,dimensions,normalize,input_bytes,input_tokens,vector,embedded_at)
         VALUES(?1,?2,?3,?4,?5,1,?6,?7,?8,?9)",
        (
            event_id,
            i64::try_from(chunk_index)?,
            i64::try_from(chunk_count)?,
            MODEL,
            DIMENSIONS_SQL,
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
    let conn = db(path)?;
    conn.execute(
        "UPDATE embedding_requests SET status='uncertain',error=?1 WHERE id=?2",
        (error, request_id),
    )?;
    Ok(())
}

fn finalize(path: &Path, event_id: &[u8], chunk_count: usize, now: i64) -> Result<(), Error> {
    let mut conn = db(path)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut statement = tx.prepare(
        "SELECT vector,input_tokens FROM embedding_chunks
         WHERE event_id=?1 AND model=?2 AND dimensions=?3 AND normalize=1
         ORDER BY chunk_index",
    )?;
    let rows: Vec<(Vec<u8>, i64)> = statement
        .query_map((event_id, MODEL, DIMENSIONS_SQL), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<Result<_, _>>()?;
    drop(statement);
    if rows.len() != chunk_count {
        return Err(format!(
            "Embedding chunks incomplete: stored={} expected={chunk_count}",
            rows.len()
        )
        .into());
    }
    let mut mean = vec![0.0_f32; DIMENSIONS];
    let mut tokens = 0_i64;
    for (bytes, input_tokens) in &rows {
        let (values, remainder) = bytes.as_chunks::<4>();
        assert!(remainder.is_empty());
        for (index, raw) in values.iter().enumerate() {
            mean[index] += f32::from_le_bytes(*raw);
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
         WHERE event_id=?1 AND model=?2 AND dimensions=?3 AND normalize=1 AND status='succeeded'",
        (event_id, MODEL, DIMENSIONS_SQL),
        |row| row.get(0),
    )?;
    tx.execute(
        "INSERT INTO post_embeddings(
           event_id,model,dimensions,normalize,chunk_count,input_tokens,cost_nusd,vector,embedded_at)
         VALUES(?1,?2,?3,1,?4,?5,?6,?7,?8)",
        (
            event_id,
            MODEL,
            DIMENSIONS_SQL,
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
/// the provider request. Titan settings follow the AWS request contract:
/// <https://docs.aws.amazon.com/bedrock/latest/userguide/model-parameters-titan-embed-text.html>.
/// -- Pi/gpt-5.6-sol
pub async fn embed_pending(
    path: &Path,
    transport: &impl Transport,
    budget: Budget,
    now: i64,
    limit: usize,
) -> Result<usize, Error> {
    let posts = pending(path, now, limit)?;
    for (event_id, text) in &posts {
        let text_chunks = chunks(text);
        for (chunk_index, text_chunk) in text_chunks.iter().enumerate() {
            let Some(request_id) =
                reserve(path, event_id, chunk_index, text_chunk.len(), budget, now)?
            else {
                continue;
            };
            // No SQLite transaction spans the network request. — Pi/gpt-5.6-sol
            let output = match transport
                .embed(Input {
                    text: text_chunk,
                    model: MODEL,
                    dimensions: DIMENSIONS,
                    normalize: NORMALIZE,
                })
                .await
            {
                Ok(output) => output,
                Err(error) => {
                    mark_uncertain(path, request_id, &error.to_string())?;
                    return Err(error);
                }
            };
            complete_request(
                path,
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
        finalize(path, event_id, text_chunks.len(), now)?;
    }
    Ok(posts.len())
}
