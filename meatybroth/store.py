"""SQLite storage for the rolling reader."""

from contextlib import contextmanager
from dataclasses import asdict, dataclass
import json
from pathlib import Path
import re
import sqlite3
import time

WINDOW_SECONDS = 30 * 86400
PUBLIC_KEY = re.compile(r"[0-9a-f]{64}")


@dataclass(frozen=True)
class Post:
    source: str
    source_id: str
    author_id: str
    author_name: str
    text: str
    created_at: int
    url: str
    parent_id: str | None = None
    root_id: str | None = None

    @property
    def canonical_id(self) -> str:
        return f"{self.source}:{self.source_id}"


def timestamp(now: int | None = None) -> int:
    return int(time.time()) if now is None else now


class Store:
    def __init__(self, path: str | Path):
        # No page cap: growth is bounded by the 30-day rolling expiry in
        # upsert/expire, not by an arbitrary SQLite max_page_count.
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        # Operator blocklist lives beside the DB (persistent volume in deploys).
        # Intentional setup ONLY for a fresh DB: a MISSING file after setup
        # means someone removed it, and load_blocklist treats that as an
        # error — never as "filtering silently off".
        self.blocklist_path = self.path.parent / "blocklist.txt"
        if not self.path.exists():
            self.blocklist_path.touch(exist_ok=True)
        with self.connect() as db:
            db.execute("PRAGMA journal_mode=WAL")
            db.executescript("""
                CREATE TABLE IF NOT EXISTS posts (
                    canonical_id TEXT PRIMARY KEY,
                    source TEXT NOT NULL,
                    source_id TEXT NOT NULL,
                    author_id TEXT NOT NULL,
                    author_name TEXT NOT NULL,
                    text TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    url TEXT NOT NULL,
                    parent_id TEXT,
                    root_id TEXT,
                    observed_at INTEGER NOT NULL,
                    UNIQUE(source, source_id)
                );
                CREATE INDEX IF NOT EXISTS posts_created ON posts(created_at DESC);
                CREATE INDEX IF NOT EXISTS posts_parent ON posts(parent_id);
                CREATE INDEX IF NOT EXISTS posts_root ON posts(root_id);
                CREATE INDEX IF NOT EXISTS posts_author ON posts(source, author_id);
                CREATE VIRTUAL TABLE IF NOT EXISTS posts_fts USING fts5(
                    text, content='posts', content_rowid='rowid', tokenize='porter unicode61'
                );
                CREATE TRIGGER IF NOT EXISTS posts_insert AFTER INSERT ON posts BEGIN
                    INSERT INTO posts_fts(rowid, text) VALUES (new.rowid, new.text);
                END;
                CREATE TRIGGER IF NOT EXISTS posts_delete AFTER DELETE ON posts BEGIN
                    INSERT INTO posts_fts(posts_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
                END;
                CREATE TRIGGER IF NOT EXISTS posts_update AFTER UPDATE ON posts BEGIN
                    INSERT INTO posts_fts(posts_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
                    INSERT INTO posts_fts(rowid, text) VALUES (new.rowid, new.text);
                END;
                CREATE TABLE IF NOT EXISTS follows (
                    follower TEXT NOT NULL, followee TEXT NOT NULL,
                    PRIMARY KEY(follower, followee)
                );
                CREATE INDEX IF NOT EXISTS follows_followee ON follows(followee);
                CREATE TABLE IF NOT EXISTS nostr_state (
                    pubkey TEXT NOT NULL, kind INTEGER NOT NULL, event_id TEXT NOT NULL,
                    created_at INTEGER NOT NULL, event_json TEXT NOT NULL, relay TEXT NOT NULL,
                    PRIMARY KEY(pubkey, kind)
                );
                CREATE TABLE IF NOT EXISTS content_warnings (
                    event_id TEXT NOT NULL, category TEXT NOT NULL,
                    reason TEXT NOT NULL, created_at INTEGER NOT NULL,
                    PRIMARY KEY(event_id, category)
                );
                CREATE TABLE IF NOT EXISTS moderation_lists (
                    source TEXT NOT NULL, identifier TEXT NOT NULL,
                    event_id TEXT NOT NULL, author TEXT NOT NULL,
                    event_created_at INTEGER NOT NULL, checked_at INTEGER NOT NULL,
                    members_json TEXT NOT NULL,
                    PRIMARY KEY(source, identifier)
                );
                CREATE TABLE IF NOT EXISTS moderation_refresh_attempts (
                    source TEXT NOT NULL, identifier TEXT NOT NULL,
                    attempted_at INTEGER NOT NULL, error TEXT,
                    PRIMARY KEY(source, identifier)
                );
                CREATE TABLE IF NOT EXISTS collection_cursors (
                    relay TEXT PRIMARY KEY, until INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS source_status (
                    source TEXT PRIMARY KEY, updated_at INTEGER NOT NULL, detail TEXT NOT NULL
                );
            """)

    @contextmanager
    def connect(self):
        db = sqlite3.connect(self.path, timeout=10)
        db.row_factory = sqlite3.Row
        try:
            with db:
                yield db
        finally:
            db.close()

    def upsert(self, post: Post, *, now: int | None = None) -> bool:
        now = timestamp(now)
        if not now - WINDOW_SECONDS <= post.created_at <= now:
            return False
        if post.source != "nostr":
            raise ValueError(f"Unsupported source: {post.source}")
        values = {"canonical_id": post.canonical_id, **asdict(post), "observed_at": now}
        with self.connect() as db:
            cursor = db.execute("""
                INSERT INTO posts(canonical_id, source, source_id, author_id, author_name,
                                  text, created_at, url, parent_id, root_id, observed_at)
                VALUES (:canonical_id, :source, :source_id, :author_id, :author_name,
                        :text, :created_at, :url, :parent_id, :root_id, :observed_at)
                ON CONFLICT(canonical_id) DO UPDATE SET
                    author_id=excluded.author_id, author_name=excluded.author_name,
                    text=excluded.text, created_at=excluded.created_at, url=excluded.url,
                    parent_id=excluded.parent_id, root_id=excluded.root_id,
                    observed_at=excluded.observed_at
                WHERE posts.author_id IS NOT excluded.author_id
                   OR posts.author_name IS NOT excluded.author_name
                   OR posts.text IS NOT excluded.text
                   OR posts.created_at IS NOT excluded.created_at
                   OR posts.url IS NOT excluded.url
                   OR posts.parent_id IS NOT excluded.parent_id
                   OR posts.root_id IS NOT excluded.root_id
            """, values)
            return cursor.rowcount > 0

    def expire(self, *, now: int | None = None) -> int:
        with self.connect() as db:
            cursor = db.execute("DELETE FROM posts WHERE created_at < ?", (timestamp(now) - WINDOW_SECONDS,))
            db.execute("""DELETE FROM content_warnings WHERE event_id NOT IN
                (SELECT source_id FROM posts WHERE source='nostr')""")
            return cursor.rowcount

    def get(self, canonical_id: str, *, now: int | None = None) -> dict | None:
        now = timestamp(now)
        with self.connect() as db:
            row = db.execute("SELECT * FROM posts WHERE canonical_id=? AND created_at BETWEEN ? AND ?",
                             (canonical_id, now - WINDOW_SECONDS, now)).fetchone()
            return dict(row) if row is not None else None

    def count(self, *, now: int | None = None) -> int:
        now = timestamp(now)
        with self.connect() as db:
            return db.execute("SELECT COUNT(*) FROM posts WHERE created_at BETWEEN ? AND ?",
                              (now - WINDOW_SECONDS, now)).fetchone()[0]

    def delete(self, canonical_id: str) -> int:
        with self.connect() as db:
            return db.execute("DELETE FROM posts WHERE canonical_id=?", (canonical_id,)).rowcount

    def save_cursor(self, relay: str, until: int) -> None:
        """Persisted backward-collection cursor: the next pass continues from
        the oldest fetched position instead of re-reading the newest pages."""
        with self.connect() as db:
            db.execute("INSERT INTO collection_cursors(relay, until) VALUES (?, ?) "
                       "ON CONFLICT(relay) DO UPDATE SET until=excluded.until", (relay, until))

    def get_cursor(self, relay: str) -> int | None:
        with self.connect() as db:
            row = db.execute("SELECT until FROM collection_cursors WHERE relay=?", (relay,)).fetchone()
            return row[0] if row else None

    def purge_metadata_with_secrets(self, find_secret) -> int:
        """Delete stored metadata containing credentials before it can render.

        The original signed event remains available from its Nostr relay; this
        database retains only the event id in collection reports, never a copy
        of the exposed credential.
        """
        removed = 0
        with self.connect() as db:
            for row in db.execute("SELECT event_id, event_json FROM nostr_state").fetchall():
                if find_secret(row["event_json"]):
                    db.execute("DELETE FROM nostr_state WHERE event_id=?", (row["event_id"],))
                    removed += 1
        return removed

    def purge_blocklisted(self, entries: set[str]) -> int:
        """Operator block action: remove stored posts by blocked event id or
        author pubkey (FTS keeps sync via delete triggers). Returns rows removed."""
        if not entries:
            return 0
        removed = 0
        with self.connect() as db:
            for entry in entries:
                for sql, args in (
                    ("DELETE FROM posts WHERE canonical_id=?", (f"nostr:{entry}",)),
                    ("DELETE FROM posts WHERE author_id=?", (entry,)),
                ):
                    removed += db.execute(sql, args).rowcount
        return removed

    def save_content_warning(self, event_id: str, category: str, reason: str, *, now: int | None = None) -> None:
        """NIP-36 author labels and spam flags kept per category (one reason per
        category: author labels and filter flags coexist); the signed body stays
        stored, the renderer decides click-to-show."""
        with self.connect() as db:
            db.execute("""INSERT INTO content_warnings(event_id, category, reason, created_at)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(event_id, category) DO UPDATE SET reason=excluded.reason""",
                       (event_id, category, reason, timestamp(now)))

    def content_warnings(self, event_ids: list[str]) -> dict[str, list[str]]:
        """event_id -> list of reasons across categories."""
        if not event_ids:
            return {}
        marks = ",".join("?" * len(event_ids))
        with self.connect() as db:
            rows = db.execute(
                f"SELECT event_id, reason FROM content_warnings WHERE event_id IN ({marks})",
                event_ids).fetchall()
        out: dict[str, list[str]] = {}
        for r in rows:
            out.setdefault(r[0], []).append(r[1])
        return out

    def save_nostr_metadata(self, event: dict, relay: str) -> bool:
        # Signature validation belongs to the collector before this call.
        kind, pubkey = event["kind"], event["pubkey"]
        if kind not in {0, 3, 10002}:
            raise ValueError(f"Unsupported metadata kind: {kind}")
        with self.connect() as db:
            changed = db.execute("""
                INSERT INTO nostr_state(pubkey, kind, event_id, created_at, event_json, relay)
                VALUES (?, ?, ?, ?, ?, ?)
                ON CONFLICT(pubkey, kind) DO UPDATE SET event_id=excluded.event_id,
                    created_at=excluded.created_at, event_json=excluded.event_json, relay=excluded.relay
                WHERE excluded.created_at > nostr_state.created_at
                   OR (excluded.created_at = nostr_state.created_at AND excluded.event_id < nostr_state.event_id)
            """, (pubkey, kind, event["id"], event["created_at"], json.dumps(event, ensure_ascii=False), relay)).rowcount
            if changed and kind == 3:
                followed = {tag[1] for tag in event["tags"]
                            if len(tag) >= 2 and tag[0] == "p" and PUBLIC_KEY.fullmatch(tag[1])}
                db.execute("DELETE FROM follows WHERE follower=?", (pubkey,))
                db.executemany("INSERT INTO follows(follower, followee) VALUES (?, ?)",
                               [(pubkey, key) for key in sorted(followed)])
            return changed > 0

    def metadata(self, pubkey: str, kind: int) -> dict | None:
        with self.connect() as db:
            row = db.execute("SELECT event_json FROM nostr_state WHERE pubkey=? AND kind=?", (pubkey, kind)).fetchone()
            return json.loads(row[0]) if row is not None else None

    def followed(self, pubkey: str) -> set[str]:
        with self.connect() as db:
            return {row[0] for row in db.execute("SELECT followee FROM follows WHERE follower=?", (pubkey,))}

    def recent_authors_missing_profiles(self, limit: int) -> list[str]:
        """Bounded kind-0 candidates, prioritizing replies used in context."""
        with self.connect() as db:
            return [row[0] for row in db.execute("""
                SELECT p.author_id
                FROM posts AS p
                WHERE p.source='nostr'
                  AND NOT EXISTS (
                    SELECT 1 FROM nostr_state AS n
                    WHERE n.pubkey=p.author_id AND n.kind=0
                  )
                GROUP BY p.author_id
                ORDER BY MAX(CASE WHEN p.parent_id IS NOT NULL OR p.root_id IS NOT NULL
                                  THEN p.created_at ELSE 0 END) DESC,
                         MAX(p.created_at) DESC
                LIMIT ?
            """, (limit,))]

    def set_moderation_list(self, source: str, identifier: str, *, event_id: str,
                            author: str, event_created_at: int, members: set[str],
                            checked_at: int) -> None:
        with self.connect() as db:
            db.execute("""INSERT INTO moderation_lists
                (source, identifier, event_id, author, event_created_at, checked_at, members_json)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(source, identifier) DO UPDATE SET
                  event_id=excluded.event_id, author=excluded.author,
                  event_created_at=excluded.event_created_at, checked_at=excluded.checked_at,
                  members_json=excluded.members_json""",
                (source, identifier, event_id, author, event_created_at, checked_at,
                 json.dumps(sorted(members))))

    def moderation_list(self, source: str, identifier: str) -> dict | None:
        with self.connect() as db:
            row = db.execute("SELECT * FROM moderation_lists WHERE source=? AND identifier=?",
                             (source, identifier)).fetchone()
        if row is None:
            return None
        return {**dict(row), "members": set(json.loads(row["members_json"]))}

    def moderation_refresh_attempt(self, source: str, identifier: str) -> dict | None:
        with self.connect() as db:
            row = db.execute("SELECT * FROM moderation_refresh_attempts WHERE source=? AND identifier=?",
                             (source, identifier)).fetchone()
        return None if row is None else dict(row)

    def record_moderation_refresh_attempt(self, source: str, identifier: str, *, attempted_at: int,
                                          error: str | None) -> None:
        with self.connect() as db:
            db.execute("""INSERT INTO moderation_refresh_attempts (source, identifier, attempted_at, error)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(source, identifier) DO UPDATE SET
                  attempted_at=excluded.attempted_at, error=excluded.error""",
                (source, identifier, attempted_at, error))

    def reconcile_primal_list(self, identifier: str, members: set[str], *, now: int) -> int:
        """Replace only this Primal category across retained Nostr posts.
        Local spam/author labels use other categories and are therefore untouched."""
        category = "primal-spam" if identifier == "spam_list" else "primal-nsfw"
        reason = ("auto-flagged: spam Primal snapshot" if identifier == "spam_list"
                  else "curated-nsfw: Primal snapshot")
        with self.connect() as db:
            db.execute("DELETE FROM content_warnings WHERE category=?", (category,))
            if not members:
                return 0
            marks = ",".join("?" for _ in members)
            rows = db.execute(
                f"SELECT canonical_id FROM posts WHERE source='nostr' AND author_id IN ({marks})",
                sorted(members)).fetchall()
            db.executemany("INSERT INTO content_warnings (event_id, category, reason, created_at) VALUES (?, ?, ?, ?)",
                           [(row["canonical_id"].removeprefix("nostr:"), category, reason, now) for row in rows])
            return len(rows)

    def set_status(self, source: str, detail: dict, *, now: int | None = None):
        with self.connect() as db:
            db.execute("""INSERT INTO source_status(source, updated_at, detail) VALUES (?, ?, ?)
                ON CONFLICT(source) DO UPDATE SET updated_at=excluded.updated_at, detail=excluded.detail""",
                       (source, timestamp(now), json.dumps(detail, ensure_ascii=False)))

    def status(self) -> list[dict]:
        with self.connect() as db:
            return [{"source": row["source"], "updated_at": row["updated_at"], **json.loads(row["detail"])}
                    for row in db.execute("SELECT * FROM source_status ORDER BY source")]
