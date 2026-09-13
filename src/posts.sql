BEGIN IMMEDIATE;
DROP VIEW IF EXISTS posts;
DROP VIEW IF EXISTS reader_post_events;

-- Keep machine presence envelopes in the canonical SDK store but outside reader and embedding eligibility. -- Pi/gpt-5.6-sol
CREATE VIEW reader_post_events AS
SELECT reader.*
FROM reader_events reader
JOIN events event ON event.id=reader.event_id
WHERE event.kind=1
  AND NOT CASE WHEN json_valid(event.content) THEN
    json_type(event.content)='object'
    AND (
      (
        json_extract(event.content,'$.type')='presence'
        AND json_type(event.content,'$.payload')='text'
        AND NOT EXISTS (
          SELECT 1 FROM json_each(event.content)
          WHERE key NOT IN ('type','payload')
        )
      )
      OR (
        json_extract(event.content,'$.type')='zone_presence'
        AND json_type(event.content,'$.zone')='text'
        AND json_type(event.content,'$.devicePk')='text'
        AND json_type(event.content,'$.role')='text'
        AND json_type(event.content,'$.metrics')='object'
        AND json_type(event.content,'$.ts')='integer'
        AND json_type(event.content,'$.ttl')='integer'
      )
    )
  ELSE 0 END;

CREATE VIEW posts AS
SELECT
    reader.id AS rowid,
    'nostr:' || lower(hex(event.id)) AS canonical_id,
    'nostr' AS source,
    lower(hex(event.id)) AS source_id,
    lower(hex(event.pubkey)) AS author_id,
    substr(lower(hex(event.pubkey)), 1, 16) AS author_name,
    event.content AS text,
    event.created_at,
    'https://njump.me/' || lower(hex(event.id)) AS url,
    reader.parent_id,
    reader.root_id
FROM events event
JOIN reader_post_events reader ON reader.event_id=event.id;
COMMIT;
