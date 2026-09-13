BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS embedding_spaces (
    id TEXT PRIMARY KEY,
    backend TEXT NOT NULL,
    model TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    normalize INTEGER NOT NULL,
    revision TEXT NOT NULL,
    model_sha256 TEXT NOT NULL,
    tokenizer_sha256 TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
-- Keep request history after event deletion because it is the durable spend ledger. -- Pi/gpt-5.6-sol
CREATE TABLE IF NOT EXISTS embedding_requests (
    id INTEGER PRIMARY KEY,
    event_id BLOB NOT NULL,
    chunk_index INTEGER NOT NULL,
    space_id TEXT NOT NULL REFERENCES embedding_spaces(id),
    requested_at INTEGER NOT NULL,
    reserved_nusd INTEGER NOT NULL,
    actual_tokens INTEGER,
    actual_nusd INTEGER,
    status TEXT NOT NULL CHECK(status IN ('reserved', 'succeeded', 'uncertain')),
    error TEXT,
    UNIQUE(event_id, chunk_index, space_id)
);
CREATE INDEX IF NOT EXISTS embedding_request_budget
ON embedding_requests(requested_at, status);
CREATE TABLE IF NOT EXISTS embedding_preflight_failures (
    request_id INTEGER PRIMARY KEY,
    event_id BLOB NOT NULL,
    chunk_index INTEGER NOT NULL,
    space_id TEXT NOT NULL,
    requested_at INTEGER NOT NULL,
    reserved_nusd INTEGER NOT NULL,
    error TEXT NOT NULL,
    archived_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS embedding_admissions (
    event_id BLOB PRIMARY KEY REFERENCES events(id) ON DELETE CASCADE,
    admitted_at INTEGER NOT NULL,
    live INTEGER NOT NULL CHECK(live IN (0,1))
);
CREATE INDEX IF NOT EXISTS embedding_admissions_queue
ON embedding_admissions(live, admitted_at, event_id);
-- Chunk and aggregate vectors follow SDK event deletion through foreign keys. -- Pi/gpt-5.6-sol
CREATE TABLE IF NOT EXISTS embedding_chunks (
    event_id BLOB NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    chunk_count INTEGER NOT NULL,
    space_id TEXT NOT NULL REFERENCES embedding_spaces(id),
    dimensions INTEGER NOT NULL,
    input_bytes INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    vector BLOB NOT NULL CHECK(length(vector) = dimensions * 4),
    embedded_at INTEGER NOT NULL,
    PRIMARY KEY(event_id, chunk_index, space_id)
);
CREATE TABLE IF NOT EXISTS embedding_queries (
    space_id TEXT NOT NULL REFERENCES embedding_spaces(id),
    query TEXT NOT NULL,
    vector BLOB NOT NULL,
    input_tokens INTEGER NOT NULL,
    cost_nusd INTEGER NOT NULL,
    embedded_at INTEGER NOT NULL,
    PRIMARY KEY(space_id, query)
);
CREATE TABLE IF NOT EXISTS post_embeddings (
    event_id BLOB NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    space_id TEXT NOT NULL REFERENCES embedding_spaces(id),
    dimensions INTEGER NOT NULL,
    chunk_count INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    cost_nusd INTEGER NOT NULL,
    vector BLOB NOT NULL CHECK(length(vector) = dimensions * 4),
    embedded_at INTEGER NOT NULL,
    PRIMARY KEY(event_id, space_id)
);
CREATE TABLE IF NOT EXISTS embedding_topics (
    space_id TEXT NOT NULL REFERENCES embedding_spaces(id),
    topic_id INTEGER NOT NULL,
    label TEXT NOT NULL,
    post_count INTEGER NOT NULL,
    centroid BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY(space_id, topic_id)
);
CREATE TABLE IF NOT EXISTS post_topics (
    event_id BLOB NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    space_id TEXT NOT NULL,
    topic_id INTEGER NOT NULL,
    PRIMARY KEY(event_id, space_id),
    FOREIGN KEY(space_id, topic_id) REFERENCES embedding_topics(space_id, topic_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS post_topics_space_topic
ON post_topics(space_id, topic_id, event_id);
COMMIT;
