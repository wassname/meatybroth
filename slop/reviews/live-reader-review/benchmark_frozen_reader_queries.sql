.timer on
.headers on
.mode tabs

SELECT 'PARAMETERS' AS section,
       1789323794 AS until_epoch,
       1786731794 AS since_epoch,
       '64c3c581f1da5a1973fb22e9bae7d91988c2298f14b92cbc3f89f1019703bbf5' AS space_id,
       101 AS page_limit,
       0 AS page_offset;

SELECT 'NEW_PLAN' AS section;
EXPLAIN QUERY PLAN
WITH eligible AS (
  SELECT p.* FROM posts p
  WHERE p.created_at BETWEEN 1786731794 AND 1789323794
), matches AS MATERIALIZED (
  SELECT canonical_id, NULL AS bm25 FROM eligible WHERE NULL IS NULL
)
SELECT p.*, x.bm25,
       0 AS n_reply_authors, 0 AS n_replies, 0 AS latest_activity,
       0 AS root_present, 0 AS hops, 0.0 AS mass
FROM eligible p
JOIN matches x USING(canonical_id)
ORDER BY p.created_at DESC, p.canonical_id ASC
LIMIT 101 OFFSET 0;

SELECT 'NEW_RESULT' AS section;
WITH eligible AS (
  SELECT p.* FROM posts p
  WHERE p.created_at BETWEEN 1786731794 AND 1789323794
), matches AS MATERIALIZED (
  SELECT canonical_id, NULL AS bm25 FROM eligible WHERE NULL IS NULL
), result AS (
  SELECT p.*, x.bm25,
         0 AS n_reply_authors, 0 AS n_replies, 0 AS latest_activity,
         0 AS root_present, 0 AS hops, 0.0 AS mass
  FROM eligible p
  JOIN matches x USING(canonical_id)
  ORDER BY p.created_at DESC, p.canonical_id ASC
  LIMIT 101 OFFSET 0
)
SELECT count(*) AS rows,
       sum(length(canonical_id) + length(source_id) + length(author_id) + length(text)) AS measured_payload_bytes
FROM result;

SELECT 'SIMILAR_VECTOR_SCAN_PLAN' AS section;
EXPLAIN QUERY PLAN
SELECT embedding.event_id, embedding.vector
FROM post_embeddings embedding
JOIN events event ON event.id = embedding.event_id
JOIN reader_post_events reader ON reader.event_id = event.id
WHERE embedding.space_id = '64c3c581f1da5a1973fb22e9bae7d91988c2298f14b92cbc3f89f1019703bbf5'
  AND event.created_at BETWEEN 1786731794 AND 1789323794;

SELECT 'SIMILAR_VECTOR_SCAN_RESULT' AS section;
SELECT count(*) AS rows,
       sum(length(embedding.event_id) + length(embedding.vector)) AS payload_bytes
FROM post_embeddings embedding
JOIN events event ON event.id = embedding.event_id
JOIN reader_post_events reader ON reader.event_id = event.id
WHERE embedding.space_id = '64c3c581f1da5a1973fb22e9bae7d91988c2298f14b92cbc3f89f1019703bbf5'
  AND event.created_at BETWEEN 1786731794 AND 1789323794;

CREATE TEMP TABLE selected_cards AS
WITH eligible AS (
  SELECT p.* FROM posts p
  WHERE p.created_at BETWEEN 1786731794 AND 1789323794
), matches AS MATERIALIZED (
  SELECT canonical_id, NULL AS bm25 FROM eligible WHERE NULL IS NULL
)
SELECT p.canonical_id, p.source_id, p.author_id, p.text, p.parent_id
FROM eligible p
JOIN matches x USING(canonical_id)
ORDER BY p.created_at DESC, p.canonical_id ASC
LIMIT 100 OFFSET 0;

SELECT 'HYDRATION_INPUT' AS section,
       count(*) AS cards,
       count(DISTINCT author_id) AS authors,
       count(DISTINCT author_id || char(0) || text) AS author_content_pairs,
       count(DISTINCT source_id) AS source_events,
       count(parent_id) AS parents
FROM selected_cards;

SELECT 'BOUNDED_REPLY_PLAN' AS section;
EXPLAIN QUERY PLAN
WITH eligible AS (
  SELECT p.* FROM posts p
  WHERE p.created_at BETWEEN 1786731794 AND 1789323794
)
SELECT parent_id, count(*)
FROM eligible
WHERE parent_id IS NOT NULL
  AND parent_id IN (SELECT canonical_id FROM selected_cards)
GROUP BY parent_id;

SELECT 'BOUNDED_REPLY_RESULT' AS section;
WITH eligible AS (
  SELECT p.* FROM posts p
  WHERE p.created_at BETWEEN 1786731794 AND 1789323794
)
SELECT count(*) AS rows
FROM (
  SELECT parent_id, count(*)
  FROM eligible
  WHERE parent_id IS NOT NULL
    AND parent_id IN (SELECT canonical_id FROM selected_cards)
  GROUP BY parent_id
);

SELECT 'IDENTITY_FULL_PLAN' AS section;
EXPLAIN QUERY PLAN
SELECT lower(hex(pubkey)), content
FROM events
WHERE kind = 0
ORDER BY created_at DESC, id ASC;

SELECT 'IDENTITY_FULL_RESULT' AS section;
SELECT count(*) AS rows,
       sum(length(lower(hex(pubkey))) + length(content)) AS payload_bytes
FROM events
WHERE kind = 0;

SELECT 'IDENTITY_BOUNDED_PLAN' AS section;
EXPLAIN QUERY PLAN
SELECT lower(hex(event.pubkey)), event.content
FROM (SELECT DISTINCT author_id FROM selected_cards) wanted
JOIN events event ON event.pubkey = unhex(wanted.author_id)
WHERE event.kind = 0
ORDER BY event.created_at DESC, event.id ASC;

SELECT 'IDENTITY_BOUNDED_RESULT' AS section;
SELECT count(*) AS rows,
       sum(length(lower(hex(event.pubkey))) + length(event.content)) AS payload_bytes
FROM (SELECT DISTINCT author_id FROM selected_cards) wanted
JOIN events event ON event.pubkey = unhex(wanted.author_id)
WHERE event.kind = 0;

SELECT 'WARNINGS_FULL_NIP36_PLAN' AS section;
EXPLAIN QUERY PLAN
SELECT lower(hex(event.id)), coalesce(json_extract(tag.value, '$[1]'), '')
FROM events event, json_each(event.tags) tag
WHERE event.kind = 1
  AND json_extract(tag.value, '$[0]') = 'content-warning';

SELECT 'WARNINGS_FULL_NIP36_RESULT' AS section;
SELECT count(*) AS rows
FROM events event, json_each(event.tags) tag
WHERE event.kind = 1
  AND json_extract(tag.value, '$[0]') = 'content-warning';

SELECT 'WARNINGS_BOUNDED_NIP36_PLAN' AS section;
EXPLAIN QUERY PLAN
SELECT lower(hex(event.id)), coalesce(json_extract(tag.value, '$[1]'), '')
FROM (SELECT DISTINCT source_id FROM selected_cards) wanted
JOIN events event ON event.id = unhex(wanted.source_id),
     json_each(event.tags) tag
WHERE event.kind = 1
  AND json_extract(tag.value, '$[0]') = 'content-warning';

SELECT 'WARNINGS_BOUNDED_NIP36_RESULT' AS section;
SELECT count(*) AS rows
FROM (SELECT DISTINCT source_id FROM selected_cards) wanted
JOIN events event ON event.id = unhex(wanted.source_id),
     json_each(event.tags) tag
WHERE event.kind = 1
  AND json_extract(tag.value, '$[0]') = 'content-warning';

SELECT 'DUPLICATES_FULL_PLAN' AS section;
EXPLAIN QUERY PLAN
SELECT lower(hex(pubkey)), content
FROM events
WHERE kind = 1
  AND created_at >= 1786731794
GROUP BY pubkey, content
HAVING count(*) > 1;

SELECT 'DUPLICATES_FULL_RESULT' AS section;
SELECT count(*) AS rows
FROM (
  SELECT pubkey, content
  FROM events
  WHERE kind = 1
    AND created_at >= 1786731794
  GROUP BY pubkey, content
  HAVING count(*) > 1
);

SELECT 'DUPLICATES_BOUNDED_PLAN' AS section;
EXPLAIN QUERY PLAN
SELECT lower(hex(event.pubkey)), event.content
FROM (SELECT DISTINCT author_id, text FROM selected_cards) wanted
CROSS JOIN events event
  ON event.pubkey = unhex(wanted.author_id)
 AND event.content = wanted.text
WHERE event.kind = 1
  AND event.created_at >= 1786731794
GROUP BY event.pubkey, event.content
HAVING count(*) > 1;

SELECT 'DUPLICATES_BOUNDED_RESULT' AS section;
SELECT count(*) AS rows
FROM (
  SELECT event.pubkey, event.content
  FROM (SELECT DISTINCT author_id, text FROM selected_cards) wanted
  CROSS JOIN events event
    ON event.pubkey = unhex(wanted.author_id)
   AND event.content = wanted.text
  WHERE event.kind = 1
    AND event.created_at >= 1786731794
  GROUP BY event.pubkey, event.content
  HAVING count(*) > 1
);
