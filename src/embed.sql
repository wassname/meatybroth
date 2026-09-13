BEGIN IMMEDIATE;
-- Keep request history after event deletion because it is the durable spend ledger. -- Pi/gpt-5.6-sol
CREATE TABLE IF NOT EXISTS embedding_requests (
    id INTEGER PRIMARY KEY,
    event_id BLOB NOT NULL,
    chunk_index INTEGER NOT NULL,
    model TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    normalize INTEGER NOT NULL,
    requested_at INTEGER NOT NULL,
    reserved_nusd INTEGER NOT NULL,
    actual_tokens INTEGER,
    actual_nusd INTEGER,
    status TEXT NOT NULL CHECK(status IN ('reserved', 'succeeded', 'uncertain')),
    error TEXT,
    UNIQUE(event_id, chunk_index, model, dimensions, normalize)
);
-- Chunk and aggregate vectors follow SDK event deletion through foreign keys. -- Pi/gpt-5.6-sol
CREATE TABLE IF NOT EXISTS embedding_chunks (
    event_id BLOB NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    chunk_count INTEGER NOT NULL,
    model TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    normalize INTEGER NOT NULL,
    input_bytes INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    vector BLOB NOT NULL CHECK(length(vector) = dimensions * 4),
    embedded_at INTEGER NOT NULL,
    PRIMARY KEY(event_id, chunk_index, model, dimensions, normalize)
);
CREATE TABLE IF NOT EXISTS post_embeddings (
    event_id BLOB NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    normalize INTEGER NOT NULL,
    chunk_count INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    cost_nusd INTEGER NOT NULL,
    vector BLOB NOT NULL CHECK(length(vector) = dimensions * 4),
    embedded_at INTEGER NOT NULL,
    PRIMARY KEY(event_id, model, dimensions, normalize)
);
COMMIT;
