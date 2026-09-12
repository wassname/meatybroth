"""Storage regressions for the reader."""

from dataclasses import replace
import sqlite3

import pytest

from meatybroth.store import Post, Store, WINDOW_SECONDS, timestamp

NOW = 1_789_200_000
ROOT = "a" * 64
FOLLOW = "b" * 64


def post(identifier="one", **changes):
    return replace(Post("nostr", identifier, ROOT, "Researcher", "activation steering", NOW - 10,
                        f"https://example.org/{identifier}"), **changes)


def integrity(store):
    with store.connect() as db:
        db.execute("INSERT INTO posts_fts(posts_fts, rank) VALUES ('integrity-check', 1)")
        assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"


def matches(store, query):
    with store.connect() as db:
        return [row[0] for row in db.execute("SELECT text FROM posts_fts WHERE posts_fts MATCH ?", (query,))]


def test_timestamp_defaults_to_current_clock(monkeypatch):
    monkeypatch.setattr("meatybroth.store.time.time", lambda: NOW)
    assert timestamp() == NOW
    assert timestamp(0) == 0


def test_replay_update_delete_and_rowid_reuse(tmp_path):
    store = Store(tmp_path / "reader.db")
    entry = post(text='activation steering ["t", "nestedtag"]')
    assert store.upsert(entry, now=NOW)
    assert not store.upsert(entry, now=NOW + 1)
    integrity(store)
    assert len(matches(store, "activation")) == 1
    assert store.upsert(replace(entry, text="representation engineering"), now=NOW)
    assert matches(store, "activation") == []
    assert matches(store, "representation") == ["representation engineering"]
    integrity(store)
    assert store.delete(entry.canonical_id) == 1
    assert matches(store, "representation") == []
    integrity(store)
    assert store.upsert(post("new"), now=NOW)
    integrity(store)


def test_literal_month_boundary_without_traffic_and_after_restart(tmp_path):
    path = tmp_path / "reader.db"
    store = Store(path)
    assert not store.upsert(post("too-old", created_at=NOW - WINDOW_SECONDS - 1), now=NOW)
    assert not store.upsert(post("future", created_at=NOW + 1), now=NOW)
    assert store.upsert(post("boundary", created_at=NOW - WINDOW_SECONDS), now=NOW)
    assert store.upsert(post("recent", created_at=NOW - WINDOW_SECONDS + 1), now=NOW)
    assert store.count(now=NOW) == 2
    assert Store(path).get("nostr:boundary", now=NOW + 1) is None
    assert Store(path).count(now=NOW + 1) == 1
    assert Store(path).expire(now=NOW + 1) == 1
    with store.connect() as db:
        assert [r[0] for r in db.execute("SELECT canonical_id FROM posts")] == ["nostr:recent"]
    integrity(store)


def test_expiring_root_keeps_recent_reply_and_missing_reference(tmp_path):
    store = Store(tmp_path / "reader.db")
    old_clock = NOW - 31 * 86400
    assert store.upsert(post("old-root", created_at=old_clock), now=old_clock)
    reply = post("reply", parent_id="nostr:old-root", root_id="nostr:old-root")
    assert store.upsert(reply, now=NOW)
    assert store.expire(now=NOW) == 1
    assert store.get("nostr:old-root", now=NOW) is None
    assert store.get("nostr:reply", now=NOW)["parent_id"] == "nostr:old-root"
    integrity(store)


def metadata(event_id, created_at, follows):
    return {"id": event_id, "pubkey": ROOT, "kind": 3, "created_at": created_at,
            "tags": [["p", pubkey] for pubkey in follows], "content": "", "sig": "fixture-only"}


def test_follow_state_is_latest_not_last_arrival_and_not_expired(tmp_path):
    store = Store(tmp_path / "reader.db")
    old = metadata("f" * 64, NOW - 200 * 86400, [FOLLOW])
    newer = metadata("e" * 64, old["created_at"] + 1, ["c" * 64])
    tie = metadata("d" * 64, newer["created_at"], ["d" * 64])
    assert store.save_nostr_metadata(old, "wss://first.example")
    assert store.save_nostr_metadata(newer, "wss://second.example")
    assert not store.save_nostr_metadata(old, "wss://stale.example")
    assert store.followed(ROOT) == {"c" * 64}
    assert store.save_nostr_metadata(tie, "wss://second.example")
    assert not store.save_nostr_metadata(newer, "wss://second.example")
    store.expire(now=NOW)
    assert store.metadata(ROOT, 3) == tie
    assert store.followed(ROOT) == {"d" * 64}
    empty = metadata("0" * 64, NOW, [])
    store.save_nostr_metadata(empty, "wss://second.example")
    assert store.followed(ROOT) == set()
    assert store.metadata(ROOT, 3) is not None


def test_no_page_cap_enforced_and_expiry_retained(tmp_path):
    """No arbitrary max_page_count: Store connections carry the SQLite default,
    so growth past the old 1GiB page bound is allowed; the 30-day expiry still
    bounds the corpus. Asserts the default without allocating gigabytes."""
    store = Store(tmp_path / "reader.db")
    assert store.upsert(post("recent"), now=NOW)
    with store.connect() as db:
        enforced = db.execute("PRAGMA max_page_count").fetchone()[0]
    raw = sqlite3.connect(tmp_path / "plain.db")
    try:
        default = raw.execute("PRAGMA max_page_count").fetchone()[0]
    finally:
        raw.close()
    assert enforced == default  # Store sets no page limit of its own
    assert enforced > 262144  # old 1GiB bound at 4KiB pages, without filling it
    assert store.expire(now=NOW + WINDOW_SECONDS + 1) == 1
    assert store.count(now=NOW + WINDOW_SECONDS + 1) == 0


def test_transaction_failure_rolls_back(tmp_path):
    store = Store(tmp_path / "reader.db")
    with pytest.raises(sqlite3.IntegrityError):
        with store.connect() as db:
            db.execute("INSERT INTO follows VALUES (?, ?)", (ROOT, FOLLOW))
            db.execute("INSERT INTO follows VALUES (?, ?)", (ROOT, FOLLOW))
    assert store.followed(ROOT) == set()


def test_source_namespace_and_status(tmp_path):
    store = Store(tmp_path / "reader.db")
    store.upsert(post("same"), now=NOW)
    # Nostr is the only ingested source now; anything else is refused.
    with pytest.raises(ValueError, match="Unsupported source"):
        store.upsert(post("same", source="unsupported"), now=NOW)
    assert store.count(now=NOW) == 1
    store.set_status("nostr", {"complete": False, "received": 10, "error": "budget exhausted"}, now=NOW)
    assert store.status() == [{"source": "nostr", "updated_at": NOW, "complete": False,
                               "received": 10, "error": "budget exhausted"}]
    assert matches(store, '"steering" AND "activations"')
    integrity(store)
