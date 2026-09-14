import argparse
import hashlib
import json
import shutil
import sqlite3
import tempfile
import time
from pathlib import Path

EXPECTED_DB_SHA256 = "cabe5372bd11729bd33468c311e9e980e5e20ddfa9bd47c973943be473cecd02"


def timed(db: sqlite3.Connection, sql: str, params=()):
    started = time.perf_counter()
    rows = db.execute(sql, params).fetchall()
    payload_bytes = sum(
        sum(len(value) for value in row if isinstance(value, (bytes, str)))
        for row in rows
    )
    return rows, {
        "seconds": time.perf_counter() - started,
        "rows": len(rows),
        "payload_bytes": payload_bytes,
    }


def show(db: sqlite3.Connection, name: str, sql: str, params=()):
    rows, result = timed(db, sql, params)
    print(name, json.dumps(result))
    print(name + "_PLAN")
    print("\n".join(row[3] for row in db.execute("EXPLAIN QUERY PLAN " + sql, params)))
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("database", type=Path)
    parser.add_argument("posts_sql", type=Path)
    args = parser.parse_args()

    observed_hash = hashlib.sha256(args.database.read_bytes()).hexdigest()
    if observed_hash != EXPECTED_DB_SHA256:
        raise ValueError(f"database SHA-256 {observed_hash} != {EXPECTED_DB_SHA256}")

    with tempfile.TemporaryDirectory() as directory:
        audit_db = Path(directory) / "latency-audit.sqlite"
        shutil.copyfile(args.database, audit_db)
        db = sqlite3.connect(audit_db)
        db.executescript(args.posts_sql.read_text())

        now = db.execute("SELECT max(created_at) FROM events").fetchone()[0]
        since = now - 30 * 86400
        space = db.execute(
            "SELECT id FROM embedding_spaces WHERE backend='bedrock' ORDER BY created_at DESC LIMIT 1"
        ).fetchone()[0]

        new_sql = """WITH eligible AS (
 SELECT p.* FROM posts p WHERE p.created_at BETWEEN :since AND :until
), matches AS MATERIALIZED (
 SELECT canonical_id,NULL AS bm25 FROM eligible WHERE :expression IS NULL
)
SELECT p.*,x.bm25,0 AS n_reply_authors,0 AS n_replies,0 AS latest_activity,
 0 AS root_present,0 AS hops,0.0 AS mass
FROM eligible p JOIN matches x USING(canonical_id)
ORDER BY p.created_at DESC,p.canonical_id ASC LIMIT :limit OFFSET :offset"""
        new_params = {
            "since": since,
            "until": now,
            "expression": None,
            "limit": 101,
            "offset": 0,
        }
        nearest_sql = """SELECT embedding.event_id,embedding.vector
FROM post_embeddings embedding
JOIN events event ON event.id=embedding.event_id
JOIN reader_post_events reader ON reader.event_id=event.id
WHERE embedding.space_id=?1 AND event.created_at BETWEEN ?2 AND ?3"""
        nearest_params = (space, since, now)

        print("snapshot_sha256=" + observed_hash)
        print("snapshot_bytes=" + str(args.database.stat().st_size))
        print(
            "params="
            + json.dumps(
                {
                    "now": now,
                    "since": since,
                    "space": space,
                    "new": new_params,
                    "nearest": nearest_params,
                },
                sort_keys=True,
            )
        )
        print(
            "cohort="
            + json.dumps(
                {
                    "eligible": db.execute(
                        "SELECT count(*) FROM posts WHERE created_at BETWEEN ? AND ?",
                        (since, now),
                    ).fetchone()[0],
                    "vectors": db.execute(
                        "SELECT count(*) FROM post_embeddings WHERE space_id=?", (space,)
                    ).fetchone()[0],
                }
            )
        )

        new_rows = show(db, "NEW", new_sql, new_params)
        show(db, "SIMILAR_VECTOR_SCAN", nearest_sql, nearest_params)

        top = new_rows[:100]
        canonical_ids = [row[1] for row in top]
        source_ids = [row[2] for row in top]
        authors = sorted({row[3] for row in top})
        pairs = list({(row[3], row[5]) for row in top})
        parents = [row[8] for row in top if row[8]]
        print(
            "HYDRATION_INPUT "
            + json.dumps(
                {
                    "cards": len(top),
                    "canonical_ids": len(canonical_ids),
                    "authors": len(authors),
                    "pairs": len(pairs),
                    "events": len(source_ids),
                    "parents": len(parents),
                }
            )
        )

        reply_sql = """WITH eligible AS (
 SELECT p.* FROM posts p WHERE p.created_at BETWEEN :since AND :until
)
SELECT parent_id,count(*) FROM eligible
WHERE parent_id IS NOT NULL AND parent_id IN (SELECT value FROM json_each(:ids))
GROUP BY parent_id"""
        show(
            db,
            "BOUNDED_REPLY",
            reply_sql,
            {"since": since, "until": now, "ids": json.dumps(canonical_ids)},
        )

        identity_full = """SELECT lower(hex(pubkey)),content FROM events
WHERE kind=0 ORDER BY created_at DESC,id ASC"""
        show(db, "IDENTITY_FULL", identity_full)
        identity_bounded = """WITH wanted(value) AS MATERIALIZED (
 SELECT value FROM json_each(?1)
)
SELECT lower(hex(e.pubkey)),e.content FROM wanted w
JOIN events e ON e.pubkey=unhex(w.value)
WHERE e.kind=0 ORDER BY e.created_at DESC,e.id ASC"""
        show(db, "IDENTITY_BOUNDED", identity_bounded, (json.dumps(authors),))

        warning_full = """SELECT lower(hex(event.id)),coalesce(json_extract(tag.value,'$[1]'),'')
FROM events event,json_each(event.tags) tag
WHERE event.kind=1 AND json_extract(tag.value,'$[0]')='content-warning'"""
        show(db, "WARNINGS_FULL_NIP36", warning_full)
        warning_bounded = """WITH wanted(value) AS MATERIALIZED (
 SELECT value FROM json_each(?1)
)
SELECT lower(hex(event.id)),coalesce(json_extract(tag.value,'$[1]'),'')
FROM wanted w JOIN events event ON event.id=unhex(w.value),json_each(event.tags) tag
WHERE event.kind=1 AND json_extract(tag.value,'$[0]')='content-warning'"""
        show(db, "WARNINGS_BOUNDED_NIP36", warning_bounded, (json.dumps(source_ids),))

        duplicates_full = """SELECT lower(hex(pubkey)),content FROM events
WHERE kind=1 AND created_at>=?1
GROUP BY pubkey,content HAVING count(*)>1"""
        show(db, "DUPLICATES_FULL", duplicates_full, (since,))
        pair_json = json.dumps([{"a": author, "c": content} for author, content in pairs])
        duplicates_bounded = """WITH wanted AS MATERIALIZED (
 SELECT unhex(json_extract(value,'$.a')) author,json_extract(value,'$.c') content
 FROM json_each(?2)
)
SELECT lower(hex(e.pubkey)),e.content FROM wanted w
JOIN events e ON e.pubkey=w.author AND e.content=w.content
WHERE e.kind=1 AND e.created_at>=?1
GROUP BY e.pubkey,e.content HAVING count(*)>1"""
        show(db, "DUPLICATES_BOUNDED", duplicates_bounded, (since, pair_json))

        print("INDEXES")
        for table in ("events", "reader_events", "post_embeddings"):
            print(table, [row[1] for row in db.execute(f"PRAGMA index_list({table})")])
        db.close()


if __name__ == "__main__":
    main()
