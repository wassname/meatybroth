-- Materialize NIP-02 p-tags once so Social reads do not rescan event JSON. -- Pi/gpt-5.6-sol
BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS social_edges (
    event_id BLOB NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    follower TEXT NOT NULL,
    followee TEXT NOT NULL,
    PRIMARY KEY(event_id, followee)
);
CREATE INDEX IF NOT EXISTS social_edges_follower ON social_edges(follower, followee);
CREATE INDEX IF NOT EXISTS social_edges_followee ON social_edges(followee, follower);
INSERT OR IGNORE INTO social_edges(event_id, follower, followee)
SELECT
    event.id,
    lower(hex(event.pubkey)),
    json_extract(tag.value, '$[1]')
FROM events event, json_each(event.tags) tag
WHERE NOT EXISTS (SELECT 1 FROM social_edges LIMIT 1)
  AND event.kind = 3
  AND json_extract(tag.value, '$[0]') = 'p'
  AND length(json_extract(tag.value, '$[1]')) = 64
  AND json_extract(tag.value, '$[1]') NOT GLOB '*[^0-9a-f]*';
DROP TRIGGER IF EXISTS social_edge_insert;
CREATE TRIGGER social_edge_insert AFTER INSERT ON event_tags
WHEN new.tag_name = 'p'
  AND length(new.tag_value) = 64
  AND new.tag_value NOT GLOB '*[^0-9a-f]*'
BEGIN
    INSERT OR IGNORE INTO social_edges(event_id, follower, followee)
    SELECT event.id, lower(hex(event.pubkey)), new.tag_value
    FROM events event
    WHERE event.id = new.event_id AND event.kind = 3;
END;
COMMIT;
