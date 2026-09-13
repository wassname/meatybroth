//! Queries the event-backed `posts` view for reader cards, threads, and social discovery.
//!
//! SQL keeps ranking and pagination in SQLite so HTTP handlers only render bounded result sets.
//! -- Pi/gpt-5.6-sol

use crate::{Error, Search};
use rusqlite::{named_params, Connection, Row};
use serde::Serialize;

/// Reader retention window in seconds.
pub const WINDOW: i64 = 30 * 86400;
/// Number of cards returned per reader page.
pub const PAGE_SIZE: usize = 50;

/// One reader card plus mode-specific ranking fields.
#[derive(Clone, Debug, Serialize)]
pub struct Post {
    pub canonical_id: String,
    pub source: String,
    pub source_id: String,
    pub author_id: String,
    pub author_name: String,
    pub text: String,
    pub created_at: i64,
    pub url: String,
    pub parent_id: Option<String>,
    pub root_id: Option<String>,
    pub bm25: Option<f64>,
    pub n_reply_authors: i64,
    pub n_replies: i64,
    pub latest_activity: i64,
    pub root_present: bool,
    pub hops: i64,
    pub mass: f64,
}

fn post(row: &Row<'_>) -> rusqlite::Result<Post> {
    Ok(Post {
        canonical_id: row.get("canonical_id")?,
        source: row.get("source")?,
        source_id: row.get("source_id")?,
        author_id: row.get("author_id")?,
        author_name: row.get("author_name")?,
        text: row.get("text")?,
        created_at: row.get("created_at")?,
        url: row.get("url")?,
        parent_id: row.get("parent_id")?,
        root_id: row.get("root_id")?,
        bm25: row.get("bm25")?,
        n_reply_authors: row.get("n_reply_authors")?,
        n_replies: row.get("n_replies")?,
        latest_activity: row.get("latest_activity")?,
        root_present: row.get("root_present")?,
        hops: row.get("hops")?,
        mass: row.get("mass")?,
    })
}

// Every ranking mode starts from the same retention and policy eligibility set. -- Pi/gpt-5.6-sol
const ELIGIBLE: &str = "WITH eligible AS (
  SELECT p.*
  FROM posts p
  WHERE p.created_at BETWEEN :since AND :until
    AND NOT EXISTS (
      SELECT 1
      FROM policy_exclusions x
      WHERE x.event_id = p.source_id
    )
    AND p.author_id NOT IN (
      SELECT m.value
      FROM moderation_lists l, json_each(l.members_json) m
      WHERE l.identifier = 'nsfw' AND m.type = 'text'
    )
)";
const CARD_DEFAULTS: &str = "0 AS n_reply_authors,
  0 AS n_replies,
  0 AS latest_activity,
  0 AS root_present,
  0 AS hops,
  0.0 AS mass";

/// Returns one bounded feed page using the selected ranking mode.
pub fn feed(db: &Connection, search: &Search, root: &str, now: i64) -> Result<Vec<Post>, Error> {
    let matches = if search.expression.is_some() {
        "SELECT p.canonical_id, bm25(posts_fts) AS bm25
         FROM posts_fts
         JOIN posts p ON p.rowid = posts_fts.rowid
         WHERE posts_fts MATCH :expression"
    } else {
        "SELECT canonical_id, NULL AS bm25
         FROM eligible
         WHERE :expression IS NULL"
    };
    let prefix = format!("{ELIGIBLE}, matches AS MATERIALIZED ({matches})");
    let sql = match search.mode.as_str() {
        // Group each stored root with its replies before ranking recent distinct repliers. -- Pi/gpt-5.6-sol
        "conversations" => format!(
            "{prefix}, members AS (
          SELECT
            *,
            coalesce(root_id, canonical_id) AS thread
          FROM eligible
        ), stats AS (
          SELECT
            m.thread,
            COUNT(DISTINCT CASE
              WHEN m.created_at >= :until - 86400
                AND m.canonical_id != m.thread
                AND (r.author_id IS NULL OR m.author_id != r.author_id)
              THEN m.source || ':' || m.author_id
            END) AS n_reply_authors,
            COUNT(*) - MAX(m.canonical_id = m.thread) AS n_replies,
            MAX(m.created_at) AS latest_activity,
            MAX(m.canonical_id = m.thread) AS root_present
          FROM members m
          LEFT JOIN eligible r
            ON r.canonical_id = m.thread
           AND coalesce(r.root_id, r.canonical_id) = m.thread
          GROUP BY m.thread
        ), cards AS (
          SELECT
            m.*,
            x.bm25,
            ROW_NUMBER() OVER (
              PARTITION BY m.thread
              ORDER BY
                CASE WHEN :expression IS NOT NULL THEN x.bm25 END ASC,
                CASE WHEN :expression IS NULL THEN m.created_at END DESC,
                CASE WHEN :expression IS NULL THEN m.canonical_id END DESC,
                m.canonical_id ASC
            ) AS position
          FROM members m
          JOIN matches x USING(canonical_id)
        )
        SELECT
          c.*,
          s.n_reply_authors,
          s.n_replies,
          s.latest_activity,
          s.root_present,
          0 AS hops,
          0.0 AS mass
        FROM cards c
        JOIN stats s ON s.thread = c.thread
        WHERE c.position = 1
        ORDER BY
          s.n_reply_authors DESC,
          s.latest_activity DESC,
          c.thread ASC"
        ),
        // NIP-02 contacts become independent first-hop endorsements after shortest-path selection. -- Pi/gpt-5.6-sol
        // Source: https://github.com/nostr-protocol/nips/blob/master/02.md
        "discovery" => format!(
            "{prefix}, edges AS MATERIALIZED (
          SELECT DISTINCT
            lower(hex(e.pubkey)) AS follower,
            json_extract(t.value, '$[1]') AS followee
          FROM events e, json_each(e.tags) t
          WHERE e.kind = 3
            AND json_extract(t.value, '$[0]') = 'p'
            AND length(json_extract(t.value, '$[1]')) = 64
            AND json_extract(t.value, '$[1]') NOT GLOB '*[^0-9a-f]*'
        ), direct AS MATERIALIZED (
          SELECT followee
          FROM edges
          WHERE follower = :root
        ), candidates AS MATERIALIZED (
          SELECT DISTINCT author_id
          FROM eligible
        ), endpoint_edges AS MATERIALIZED (
          SELECT *
          FROM edges
          WHERE followee IN (SELECT author_id FROM candidates)
        ), paths AS (
          SELECT followee AS author, followee AS endorser, 1 AS distance
          FROM direct
          WHERE followee IN (SELECT author_id FROM candidates)
          UNION ALL
          SELECT e.followee, d.followee, 2
          FROM direct d
          JOIN endpoint_edges e ON e.follower = d.followee
          WHERE e.followee != :root AND e.followee != d.followee
          UNION ALL
          SELECT h.followee, d.followee, 3
          FROM direct d
          JOIN edges e ON e.follower = d.followee
          JOIN endpoint_edges h ON h.follower = e.followee
          WHERE e.followee != :root
            AND e.followee != d.followee
            AND e.followee NOT IN (SELECT followee FROM direct)
            AND h.followee != :root
            AND h.followee != d.followee
        ), shortest AS (
          SELECT author, endorser, MIN(distance) AS distance
          FROM paths
          GROUP BY author, endorser
        ), reach AS (
          SELECT
            author,
            MIN(distance) AS hops,
            SUM(distance = 1) + SUM(distance = 2) * 0.25
              + SUM(distance = 3) * (1.0 / 9) AS mass
          FROM shortest
          GROUP BY author
        )
        SELECT
          p.*,
          x.bm25,
          0 AS n_reply_authors,
          0 AS n_replies,
          0 AS latest_activity,
          0 AS root_present,
          r.hops,
          r.mass
        FROM eligible p
        JOIN matches x USING(canonical_id)
        JOIN reach r ON r.author = p.author_id
        WHERE r.hops <= :reach
        ORDER BY {}p.created_at DESC, p.canonical_id ASC",
            if search.order == "connections" {
                "r.mass DESC, "
            } else {
                ""
            }
        ),
        _ => format!(
            "{prefix}
          SELECT
            p.*,
            x.bm25,
            {CARD_DEFAULTS}
          FROM eligible p
          JOIN matches x USING(canonical_id)
          ORDER BY {}p.created_at DESC, p.canonical_id ASC",
            if search.mode == "relevance" {
                "x.bm25 ASC, "
            } else {
                ""
            }
        ),
    };
    let mut stmt = db.prepare(&format!("{sql} LIMIT :limit OFFSET :offset"))?;
    let params = named_params! {
        ":since": now - WINDOW,
        ":until": search.before.unwrap_or(now),
        ":expression": search.expression,
        ":limit": (PAGE_SIZE + 1) as i64,
        ":offset": search.page.saturating_mul(PAGE_SIZE as i64),
    };
    for (name, value) in params {
        stmt.raw_bind_parameter(stmt.parameter_index(name)?.unwrap(), *value)?;
    }
    if search.mode == "discovery" {
        stmt.raw_bind_parameter(stmt.parameter_index(":root")?.unwrap(), root)?;
        stmt.raw_bind_parameter(stmt.parameter_index(":reach")?.unwrap(), search.reach)?;
    }
    let mut rows = stmt.raw_query();
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(post(row)?);
    }
    Ok(result)
}

/// Gets one eligible card by canonical ID.
pub fn get(db: &Connection, id: &str, now: i64) -> Result<Option<Post>, Error> {
    let mut stmt = db.prepare(&format!(
        "{ELIGIBLE}
         SELECT
           *,
           NULL AS bm25,
           {CARD_DEFAULTS}
         FROM eligible
         WHERE canonical_id = :id"
    ))?;
    let mut rows = stmt.query(named_params! {":since": now - WINDOW, ":until": now, ":id": id})?;
    Ok(rows.next()?.map(post).transpose()?)
}

/// Counts eligible posts, or direct replies when `parent` is set.
pub fn count(db: &Connection, now: i64, parent: Option<&str>) -> Result<i64, Error> {
    let condition = if parent.is_some() {
        "parent_id=:parent"
    } else {
        ":parent IS NULL"
    };
    Ok(db.query_row(
        &format!("{ELIGIBLE}\nSELECT count(*)\nFROM eligible\nWHERE {condition}"),
        named_params! {":since": now - WINDOW, ":until": now, ":parent": parent},
        |r| r.get(0),
    )?)
}

/// Returns direct replies in deterministic chronological order.
pub fn children(db: &Connection, id: &str, now: i64) -> Result<Vec<Post>, Error> {
    let mut stmt = db.prepare(&format!(
        "{ELIGIBLE}
         SELECT
           *,
           NULL AS bm25,
           {CARD_DEFAULTS}
         FROM eligible
         WHERE parent_id = :id
         ORDER BY created_at, canonical_id"
    ))?;
    let rows = stmt
        .query_map(
            named_params! {":since": now - WINDOW, ":until": now, ":id": id},
            post,
        )?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}
