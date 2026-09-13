BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS collection_cursors (
 relay TEXT PRIMARY KEY,
 forward_at INTEGER NOT NULL,
 backfill_before INTEGER NOT NULL,
 updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS collection_runs (
 id INTEGER PRIMARY KEY,
 relay TEXT NOT NULL,
 phase TEXT NOT NULL CHECK(phase IN ('recent','forward','backfill')),
 since_at INTEGER NOT NULL,
 until_at INTEGER NOT NULL,
 started_at INTEGER NOT NULL,
 finished_at INTEGER,
 eose INTEGER,
 accepted INTEGER,
 rejected INTEGER
);
CREATE INDEX IF NOT EXISTS collection_run_source ON collection_runs(relay,id);
DROP VIEW IF EXISTS source_status;
CREATE VIEW source_status AS
 SELECT r.relay AS source,coalesce(r.finished_at,r.started_at) AS updated_at,
 json_object('phase',r.phase,'since_at',r.since_at,'until_at',r.until_at,
             'eose',r.eose,'accepted',r.accepted,'rejected',r.rejected,
             'coverage',CASE WHEN r.finished_at IS NULL THEN 'scan interrupted; gap retained'
                             WHEN EXISTS(SELECT 1 FROM collection_gaps g WHERE g.relay=r.relay AND g.since_at=r.since_at AND g.until_at=r.until_at) THEN 'EOSE without inventory proof; gap retained'
                             WHEN r.eose=1 THEN 'bounded interval reconciled'
                             ELSE 'coverage not established' END) AS detail
 FROM collection_runs r
 WHERE r.id=(SELECT max(last.id) FROM collection_runs last WHERE last.relay=r.relay)
 UNION ALL
 SELECT 'primal-moderation',max(attempted_at),
 json_object('coverage','signed snapshot refresh','last_error',
             (SELECT error FROM moderation_refresh_attempts ORDER BY attempted_at DESC,rowid DESC LIMIT 1))
 FROM moderation_refresh_attempts HAVING count(*)>0;
COMMIT;
