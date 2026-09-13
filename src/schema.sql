BEGIN IMMEDIATE;
CREATE TABLE reader_events (
 id INTEGER PRIMARY KEY,
 event_id BLOB NOT NULL UNIQUE REFERENCES events(id) ON DELETE CASCADE,
 received_at INTEGER NOT NULL DEFAULT (unixepoch()),
 root_id TEXT,
 parent_id TEXT
);
CREATE INDEX reader_parent ON reader_events(parent_id);
CREATE INDEX reader_root ON reader_events(root_id);
CREATE VIEW posts AS SELECT r.id AS rowid,'nostr:'||lower(hex(e.id)) AS canonical_id,
 'nostr' AS source,lower(hex(e.id)) AS source_id,lower(hex(e.pubkey)) AS author_id,
 substr(lower(hex(e.pubkey)),1,16) AS author_name,e.content AS text,e.created_at,
 'https://njump.me/'||lower(hex(e.id)) AS url,r.parent_id,r.root_id
 FROM events e JOIN reader_events r ON r.event_id=e.id WHERE e.kind=1;
CREATE VIRTUAL TABLE posts_fts USING fts5(text,content='posts',content_rowid='rowid',tokenize='porter unicode61');
CREATE TRIGGER reader_insert AFTER INSERT ON events BEGIN
 INSERT INTO reader_events(event_id,root_id,parent_id)
 SELECT new.id,'nostr:'||coalesce(root,reply),'nostr:'||coalesce(reply,root) FROM (
  WITH refs AS (
   SELECT value,key FROM json_each(new.tags) WHERE new.kind=1 AND json_extract(value,'$[0]')='e'
    AND length(json_extract(value,'$[1]'))=64 AND json_extract(value,'$[1]') NOT GLOB '*[^0-9a-f]*'
  ) SELECT
   coalesce((SELECT json_extract(value,'$[1]') FROM refs WHERE json_extract(value,'$[3]')='root' ORDER BY key LIMIT 1),
            (SELECT json_extract(value,'$[1]') FROM refs WHERE json_extract(value,'$[3]') IS NULL ORDER BY key LIMIT 1)) AS root,
   coalesce((SELECT json_extract(value,'$[1]') FROM refs WHERE json_extract(value,'$[3]')='reply' ORDER BY key LIMIT 1),
            (SELECT json_extract(value,'$[1]') FROM refs WHERE json_extract(value,'$[3]') IS NULL ORDER BY key DESC LIMIT 1)) AS reply
 );
 INSERT INTO posts_fts(rowid,text) SELECT id,new.content FROM reader_events WHERE event_id=new.id AND new.kind=1;
END;
CREATE TRIGGER reader_delete BEFORE DELETE ON events WHEN old.kind=1 BEGIN
 INSERT INTO posts_fts(posts_fts,rowid,text) SELECT 'delete',id,old.content FROM reader_events WHERE event_id=old.id;
END;
CREATE VIEW moderation_lists AS SELECT 'primal' AS source,
 replace(json_extract(d.value,'$[1]'),'_list','') AS identifier,
 lower(hex(e.id)) AS event_id,e.created_at AS event_created_at,r.received_at AS checked_at,
 (SELECT json_group_array(json_extract(p.value,'$[1]')) FROM json_each(e.tags) p
  WHERE json_extract(p.value,'$[0]')='p' AND length(json_extract(p.value,'$[1]'))=64
   AND json_extract(p.value,'$[1]') NOT GLOB '*[^0-9a-f]*') AS members_json
 FROM events e JOIN reader_events r ON r.event_id=e.id,json_each(e.tags) d
 WHERE e.kind=30000 AND lower(hex(e.pubkey))='{primal_author}'
  AND json_extract(d.value,'$[0]')='d' AND json_extract(d.value,'$[1]') IN ('nsfw_list','spam_list');
CREATE VIEW content_warnings AS SELECT lower(hex(e.id)) AS event_id,'primal-spam' AS category,
 'auto-flagged: spam Primal snapshot' AS reason FROM events e
 WHERE e.kind=1 AND lower(hex(e.pubkey)) IN (
  SELECT m.value FROM moderation_lists l,json_each(l.members_json) m WHERE l.identifier='spam'
 );
CREATE TABLE policy_exclusions(event_id TEXT PRIMARY KEY,reason TEXT NOT NULL);
CREATE TABLE collection_gaps(relay TEXT,since_at INTEGER,until_at INTEGER,reason TEXT,checked_at INTEGER);
CREATE TABLE moderation_refresh_attempts(source TEXT,identifier TEXT,attempted_at INTEGER,error TEXT);
CREATE VIEW source_status AS SELECT 'sdk' AS source,max(received_at) AS updated_at,
 json_object('stored_notes',(SELECT count(*) FROM posts),'coverage','not yet established',
             'moderation_checked_at','first local observation; not a refresh claim') AS detail
 FROM reader_events HAVING count(*)>0;
COMMIT;
