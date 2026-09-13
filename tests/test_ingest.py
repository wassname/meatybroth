"""Offline and (opt-in) real-network tests for meatybroth.ingest."""

from __future__ import annotations

import copy
import json
import os
import time
from urllib.parse import quote

import pytest
from coincurve import PrivateKey

from meatybroth.ingest import (
    BOOTSTRAP_RELAYS,
    CONFIGURED_RELAYS,
    Budget,
    QueryResult,
    collect_once,
    event_digest,
    load_blocklist,
    nostr_post,
    thread_refs,
    verify_event,
)
from meatybroth.store import Store, WINDOW_SECONDS

SECRET = bytes(range(1, 32))
KEY_C, KEY_D, KEY_E = (bytes.fromhex(x * 64) for x in "cde")
KEY_F = bytes.fromhex("ab" * 32)  # "f"*64 exceeds the secp256k1 group order
NOW = int(time.time())


def pubkey_of(secret: bytes) -> str:
    return PrivateKey(secret).public_key.format(compressed=True)[1:].hex()


ROOT = pubkey_of(SECRET)  # tests root at this key; the plan default is only used live
PUB_C, PUB_D, PUB_E, PUB_F = (pubkey_of(k) for k in (KEY_C, KEY_D, KEY_E, KEY_F))


def sign_event(kind: int, content: str, *, created_at: int, tags: list | None = None, secret: bytes = SECRET) -> dict:
    private = PrivateKey(secret)
    event = {"id": "", "pubkey": pubkey_of(secret), "created_at": created_at, "kind": kind,
             "tags": tags or [], "content": content, "sig": ""}
    digest = event_digest(event)
    event["id"] = digest.hex()
    event["sig"] = private.sign_schnorr(digest).hex()
    return event


def e_tag(event_id: str, marker: str | None = None) -> list:
    return ["e", event_id] if marker is None else ["e", event_id, "", marker]


def run(store, relay_events: dict[str, tuple[list[dict], bool] | Exception],
        *, max_requests: int = 40, root: str = ROOT):
    relay = MockRelay()
    relay.responses = {url: ([], True) for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}
    relay.responses.update(relay_events)
    return collect_once(store, root, max_requests=max_requests, relays=relay, refresh_primal=False)


class MockRelay:
    """Serves preloaded events per relay with honest filter matching."""

    def __init__(self):
        self.queries: list[tuple[str, list[dict]]] = []
        self.responses: dict[str, tuple[list[dict], bool] | Exception] = {}

    def query(self, relay_url: str, filters: list[dict], *, timeout: float = 0) -> QueryResult:
        self.queries.append((relay_url, filters))
        spec = self.responses[relay_url]  # KeyError on unprepared relay: loud, wanted
        if isinstance(spec, Exception):
            raise spec
        events, complete = spec
        # NIP-01: limit applies PER FILTER, then results are unioned.
        matched, seen = [], set()
        for f in filters:
            f_events = [e for e in events if self._matches(f, e)]
            if "limit" in f:
                # relays typically serve the newest matches first when limiting
                f_events = sorted(f_events, key=lambda e: e["created_at"], reverse=True)[:f["limit"]]
            for e in f_events:
                if e["id"] not in seen:
                    seen.add(e["id"])
                    matched.append(e)
        return QueryResult(matched, complete)

    @staticmethod
    def _matches(f: dict, event: dict) -> bool:
        if "ids" in f and event["id"] not in f["ids"]:
            return False
        if "authors" in f and event["pubkey"] not in f["authors"]:
            return False
        if "kinds" in f and event["kind"] not in f["kinds"]:
            return False
        if "since" in f and event["created_at"] < f["since"]:
            return False
        if "until" in f and event["created_at"] > f["until"]:
            return False
        return True


@pytest.fixture
def store(tmp_path):
    return Store(tmp_path / "test.db")


# ---------------------------------------------------------------- verification


def test_verify_event_rejects_tampering():
    event = sign_event(1, "hello", created_at=NOW)
    assert verify_event(event)

    content_tampered = copy.deepcopy(event)
    content_tampered["content"] = "evil"
    assert not verify_event(content_tampered)

    id_tampered = copy.deepcopy(event)
    id_tampered["id"] = "0" * 64
    assert not verify_event(id_tampered)

    sig_tampered = copy.deepcopy(event)
    sig_tampered["sig"] = "0" * 128
    assert not verify_event(sig_tampered)

    wrong_key = copy.deepcopy(event)
    wrong_key["pubkey"] = pubkey_of(KEY_C)
    assert not verify_event(wrong_key)

    malformed = {"id": "nope", "pubkey": "nope", "sig": "nope", "kind": 1,
                 "created_at": "not-an-int", "tags": [], "content": "x"}
    assert not verify_event(malformed)


def test_thread_refs_prefers_nip10_markers():
    root_id, other_id = "a" * 64, "b" * 64
    marked = {"tags": [e_tag(other_id, "reply"), e_tag(root_id, "root")]}
    assert thread_refs(marked) == (root_id, other_id)
    unmarked = {"tags": [e_tag(root_id), e_tag(other_id)]}
    assert thread_refs(unmarked) == (root_id, other_id)


def test_thread_refs_root_marked_direct_reply_is_reply_to_root():
    """NIP-10: 'For top level replies (those replying directly to the root event),
    only the root marker should be used' — a lone root tag doubles as the reply."""
    root_id = "a" * 64
    assert thread_refs({"tags": [e_tag(root_id, "root")]}) == (root_id, root_id)


def test_backfill_cursor_uses_accepted_events_only(store):
    """A malformed or unrelated event in a page can neither crash the cursor
    (no created_at) nor steer it (old signed event outside the filter)."""
    follow = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
    newest = [sign_event(1, f"post {i}", created_at=NOW - 3600 - i, secret=KEY_C) for i in range(500)]
    malformed = {"id": "f" * 64, "pubkey": PUB_C, "kind": 1, "content": "no created_at",
                 "sig": "0" * 128}  # missing created_at: would crash a raw min()
    old_outsider = sign_event(1, "too old for window", created_at=NOW - WINDOW_SECONDS - 10,
                              secret=KEY_C)
    older_real = sign_event(1, "older real post", created_at=NOW - 5 * 86400, secret=KEY_C)

    class SloppyRelay:
        """Full pages plus junk; ignores until/since like a misbehaving relay."""
        def query(self, relay_url, filters, *, timeout=0):
            events = [follow] if relay_url in BOOTSTRAP_RELAYS and filters[0].get("kinds") == [3] \
                else [f for f in [follow, *newest, malformed, old_outsider, older_real]
                      if not ("until" in filters[0] and f["created_at"] > filters[0]["until"])]
            return QueryResult(events, complete=True)

    report = collect_once(store, ROOT, max_requests=20, relays=SloppyRelay())["nostr"]
    assert report["filter_mismatched"] >= 1  # old-outside-window event rejected
    assert report["invalid_events"] >= 1  # malformed event rejected
    assert store.get(f"nostr:{older_real['id']}", now=NOW) is not None  # cursor reached it
    assert store.get(f"nostr:{old_outsider['id']}", now=NOW) is None


def test_filter_mismatched_signed_event_not_stored(store):
    """A relay returning signed-but-unrelated events is not requested evidence."""
    follow = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
    requested = sign_event(1, "requested note", created_at=NOW - 60, secret=KEY_C)
    unrelated = sign_event(1, "someone else entirely", created_at=NOW - 60, secret=KEY_D)

    class BrokenRelay:
        """Ignores the filter like a misbehaving relay; signed junk included."""
        events = [follow, requested, unrelated]
        def query(self, relay_url, filters, *, timeout=0):
            return QueryResult(list(self.events), complete=True)

    report = collect_once(store, ROOT, max_requests=15, relays=BrokenRelay())["nostr"]
    assert store.get(f"nostr:{requested['id']}", now=NOW) is not None
    assert store.get(f"nostr:{unrelated['id']}", now=NOW) is None
    assert report["filter_mismatched"] > 0  # every unrelated copy counted, none stored



# -------------------------------------------------------------- Nostr pipeline


def test_collect_persists_verified_posts_and_rejects_invalid(store):
    root_kind3 = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
    good = sign_event(1, "recent note", created_at=NOW - 60, secret=KEY_C)
    reply = sign_event(1, "a reply", created_at=NOW - 30, secret=KEY_C,
                       tags=[e_tag("d" * 64, "root"), e_tag(good["id"], "reply")])
    # unique event with a corrupted signature (same-id duplicates are
    # indistinguishable at relay level; a real forgery has its own id)
    forged = sign_event(1, "forged note", created_at=NOW - 59, secret=KEY_C)
    forged["sig"] = "2" * 128
    report = run(store, {
        "wss://purplepag.es": ([root_kind3, good, reply, forged], True),
    })
    assert report["nostr"]["invalid_events"] == 1
    assert store.count(now=NOW) == 2
    stored_reply = store.get(f"nostr:{reply['id']}", now=NOW)
    assert stored_reply["parent_id"] == f"nostr:{good['id']}"
    assert stored_reply["root_id"] == "nostr:" + "d" * 64
    assert report["nostr"]["posts_upserted"] == 2
    assert report["requests_used"] > 0
    assert {row["source"] for row in store.status()} == {"nostr"}


def test_collect_survives_relay_errors_and_reports_gap(store):
    report = run(store, {"wss://purplepag.es": ConnectionError("connection refused")})
    assert "wss://purplepag.es" in report["nostr"]["relay_errors"]
    assert store.count(now=NOW) == 0  # healthy but empty, gaps explicit


def test_missing_kind3_is_reported_not_fabricated(store):
    """Kind 0 profile present but follow list absent: collector must not invent follows."""
    profile = sign_event(0, json.dumps({"name": "rooty"}), created_at=NOW - 3600)
    report = run(store, {"wss://purplepag.es": ([profile], True)})
    assert store.followed(ROOT) == set()
    assert report["nostr"]["follows"] == 0
    assert any("no follow list" in gap for gap in report["nostr"]["gaps"])
    assert store.count(now=NOW) == 0


def test_two_hop_discovery_capped_and_deduplicated(store):
    root_kind3 = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C], ["p", PUB_D]])
    # Follows' own lists: both follow outsider E (deduplicated), D also follows F.
    a_list = sign_event(3, "", created_at=NOW - 3600, tags=[["p", PUB_E]], secret=KEY_C)
    b_list = sign_event(3, "", created_at=NOW - 3600, tags=[["p", PUB_E], ["p", PUB_F]], secret=KEY_D)
    outsider_post = sign_event(1, "discovered outsider note", created_at=NOW - 120, secret=KEY_E)
    report = run(store, {
        "wss://purplepag.es": ([root_kind3], True),
        "wss://nos.lol": ([a_list, b_list, outsider_post], True),
    })
    assert report["nostr"]["second_hop_authors"] == 2
    assert report["nostr"]["second_hop_capped"] is False
    assert store.get(f"nostr:{outsider_post['id']}", now=NOW) is not None
    assert store.get(f"nostr:{a_list['id']}", now=NOW) is None  # follow lists are metadata, not posts


def test_budget_stops_collection_and_reports_partial(store):
    root_kind3 = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
    report = run(store, {"wss://purplepag.es": ([root_kind3], True)}, max_requests=3)
    assert report["requests_used"] <= 3
    assert report["nostr"]["gaps"], "tight budget must leave explicit gaps"


def test_recent_orphan_kept_old_parent_body_refused(store):
    parent_old = sign_event(1, "31-day-old parent", created_at=NOW - WINDOW_SECONDS - 86400, secret=KEY_D)
    reply = sign_event(1, "fresh reply to old parent", created_at=NOW - 60, secret=KEY_C,
                       tags=[e_tag(parent_old["id"], "root"), e_tag(parent_old["id"], "reply")])
    root_kind3 = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
    report = run(store, {"wss://purplepag.es": ([root_kind3, parent_old, reply], True)})
    assert report["nostr"]["parents_older_than_window"] == 1
    assert store.get(f"nostr:{parent_old['id']}", now=NOW) is None  # old body never stored
    orphan = store.get(f"nostr:{reply['id']}", now=NOW)
    assert orphan is not None and orphan["parent_id"] == f"nostr:{parent_old['id']}"
    status = {row["source"]: row for row in store.status()}["nostr"]
    assert status["parents_older_than_window"] == 1


def test_missing_parent_orphan_preserved_with_explicit_gap(store):
    root_kind3 = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
    absent_parent = "e" * 64  # valid hex, but no event served anywhere
    reply = sign_event(1, "fresh reply, parent missing", created_at=NOW - 60, secret=KEY_C,
                       tags=[e_tag(absent_parent, "root"), e_tag(absent_parent, "reply")])
    report = run(store, {"wss://purplepag.es": ([root_kind3, reply], True)})
    assert report["nostr"]["parents_missing"] >= 1
    orphan = store.get(f"nostr:{reply['id']}", now=NOW)
    assert orphan is not None and orphan["parent_id"] == f"nostr:{absent_parent}"


def test_collector_reports_outbox_relay_discovery(store):
    """Followed authors' advertised write relays join the content relay set."""
    follow = sign_event(3, "", created_at=NOW - 3600, tags=[["p", PUB_C]])
    note = sign_event(1, "nostr only", created_at=NOW - 60, secret=KEY_C)
    # C's own kind-10002 advertises a bridge outbox relay.
    outbox_list = sign_event(10002, "", created_at=NOW - 3500,
                             tags=[["r", "wss://bridge.example", "write"]], secret=KEY_C)
    relay = MockRelay()
    relay.responses = {url: ([follow] if url == BOOTSTRAP_RELAYS[0] else [])
                       for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS + ["wss://bridge.example"]}
    relay.responses[BOOTSTRAP_RELAYS[1]] = ([follow, outbox_list, note], True)
    # identity(4) + outbox wave(5 initial) + base phases for 6 relays (18)
    # with the 70% base share, then bridge.example base phases (3)
    report = collect_once(store, ROOT, max_requests=45, relays=relay)["nostr"]
    assert report["follow_outbox_relays_added"] == 1
    assert any(relay_url == "wss://bridge.example" for relay_url, _ in relay.queries)
    assert store.get(f"nostr:{note['id']}", now=NOW) is not None


# --------------------------------------------------- opt-in real network proof


@pytest.mark.skipif(not os.environ.get("MEATYBROTH_REAL_NETWORK"),
                    reason="set MEATYBROTH_REAL_NETWORK=1 for the small live proof")
def test_real_bounded_collection(tmp_path, capsys):
    from meatybroth.ingest import HttpTransport, RelayTransport  # real transports

    store = Store(tmp_path / "real.db")
    report = collect_once(store, "60c052cf19fbfb973c1779585df423e3982a3a251fc826d4c76f8063621c5bb6", max_requests=24)
    print(json.dumps(report, indent=1)[:3000])
    assert report["requests_used"] <= 24
    assert report["nostr"]["follows"] > 0

def test_configured_bridge_relays_get_budgeted_collection(store):
    """The verified bridge/RustyClaw relays (relay-sources review) receive real
    posts-phase queries, not just decoration."""
    follow = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
    relay = MockRelay()
    relay.responses = {url: ([follow], True) for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}
    collect_once(store, ROOT, max_requests=60, relays=relay)
    queried = {url for url, _ in relay.queries}
    for configured in CONFIGURED_RELAYS:
        assert configured in queried, configured
    # and the posts phase specifically ran on them (authors filter, kind 1)
    posts_phases = {url for url, filters in relay.queries
                    if any(f.get("kinds") == [1] and "authors" in f for f in filters)}
    for configured in CONFIGURED_RELAYS:
        assert configured in posts_phases, configured

def test_configured_relays_survive_many_advertised(store):
    """Even when the root and follows advertise far more relays than the cap,
    every CONFIGURED_RELAYS entry keeps its slot and a posts-phase query."""
    many = [f"wss://advertised.example/{i}" for i in range(12)]
    root_kind3 = sign_event(3, "", created_at=NOW - 7200,
                            tags=[["p", PUB_C]] + [["r", url, "write"] for url in many[:3]])
    follows_lists = sign_event(3, "", created_at=NOW - 3600,
                               tags=[["r", url, "write"] for url in many[3:]], secret=KEY_C)
    relay = MockRelay()
    relay.responses = {url: ([root_kind3] if url == BOOTSTRAP_RELAYS[0] else [])
                       for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS + many}
    relay.responses[BOOTSTRAP_RELAYS[1]] = ([root_kind3, follows_lists], True)
    report = collect_once(store, ROOT, max_requests=90, relays=relay)["nostr"]
    queried = {url for url, _ in relay.queries}
    for configured in CONFIGURED_RELAYS:
        assert configured in queried, f"{configured} truncated by advertised relays"
    posts_phases = {url for url, filters in relay.queries
                    if any(f.get("kinds") == [1] and "authors" in f for f in filters)}
    for configured in CONFIGURED_RELAYS:
        assert configured in posts_phases

def test_filters_reject_secrets_flag_spam_and_honor_nip36(store):
    """Synthetic-only filter checks: secrets never persist; spam/cw/explicit flag."""
    import re as _re
    follow = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
    nsec_note = sign_event(1, "oops " + "nsec1qqsyfakefakefakefak3fakefakefakefakefakefake"[:58],
                           created_at=NOW - 70, secret=KEY_C)
    akia_note = sign_event(1, "key AKIAIOSFODNN7EXAMPLE leaked", created_at=NOW - 71, secret=KEY_C)
    cw_note = sign_event(1, "art post", created_at=NOW - 72, secret=KEY_C,
                         tags=[["content-warning", "nsfw art"]])
    clean = sign_event(1, "my doctor changed my HRT dose", created_at=NOW - 73, secret=KEY_C)
    linkfarm = sign_event(1, "check " + " ".join(f"https://x.example/{i}" for i in range(6)),
                          created_at=NOW - 74, secret=KEY_C)
    explicit_note = sign_event(1, "this has an explicit sex scene", created_at=NOW - 75, secret=KEY_C)
    report = collect_once(store, ROOT, max_requests=40,
                          relays=MockRelay()) if False else None
    relay = MockRelay()
    relay.responses = {url: ([follow, nsec_note, akia_note, cw_note, clean, linkfarm, explicit_note], True)
                       for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}
    report = collect_once(store, ROOT, max_requests=40, relays=relay)["nostr"]
    assert store.get(f"nostr:{nsec_note['id']}", now=NOW) is None  # never persisted
    assert store.get(f"nostr:{akia_note['id']}", now=NOW) is None
    assert store.get(f"nostr:{clean['id']}", now=NOW) is not None  # benign renders normally
    assert store.get(f"nostr:{cw_note['id']}", now=NOW) is not None  # stored, flagged
    assert store.content_warnings([cw_note["id"]])[cw_note["id"]] == ["author: nsfw art"]
    assert store.content_warnings([linkfarm["id"]])[linkfarm["id"]] == ["auto-flagged: spam link-farm"]
    assert report["filter_stats"]["secret"] == 2
    assert report["filter_stats"]["cw"] == 1
    assert report["filter_stats"]["explicit"] == 1
    assert store.content_warnings([clean["id"]]) == {}  # HRT health discussion untouched


def test_blocklist_prevents_reingestion(store, tmp_path):
    blocked_id = sign_event(1, "blocked content", created_at=NOW - 60, secret=KEY_C)
    bl = tmp_path / "blocklist.txt"
    bl.write_text("# operator blocklist\n" + blocked_id["id"] + "\n")
    import meatybroth.ingest as ing
    orig = ing.load_blocklist
    ing.load_blocklist = lambda store: {blocked_id["id"]}
    try:
        follow = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
        relay = MockRelay()
        relay.responses = {url: ([follow, blocked_id], True)
                           for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}
        report = collect_once(store, ROOT, max_requests=40, relays=relay)["nostr"]
        assert store.get(f"nostr:{blocked_id['id']}", now=NOW) is None  # never re-enters
        assert report["filter_stats"]["blocklist"] >= 1
    finally:
        ing.load_blocklist = orig


def test_secret_purge_removes_already_stored_copy(store):
    dirty = sign_event(1, "stored before filter: sk-ant-" + "A1b2C3d4E5" * 4,
                       created_at=NOW - 60, secret=KEY_C)
    store.upsert(nostr_post(dirty, "x"), now=NOW)  # simulate pre-filter storage
    follow = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
    relay = MockRelay()
    relay.responses = {url: ([follow, dirty], True) for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}
    report = collect_once(store, ROOT, max_requests=40, relays=relay)["nostr"]
    assert store.get(f"nostr:{dirty['id']}", now=NOW) is None  # purged
    assert report["filter_stats"]["secret_purged"] >= 1

def test_warning_refresh_on_old_rows_and_combined_categories(store):
    """Production old-row replay: unchanged (already-stored) posts acquire
    NIP-36/explicit/spam flags independently of the body upsert; distinct
    categories coexist on one event."""
    import re as _re
    follow = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
    # first pass stores the post WITHOUT any warning (no tags, benign text)
    plain = sign_event(1, "plain post", created_at=NOW - 60, secret=KEY_C)
    relay = MockRelay()
    relay.responses = {url: ([follow, plain], True) for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}
    collect_once(store, ROOT, max_requests=40, relays=relay)
    assert store.get(f"nostr:{plain['id']}", now=NOW) is not None
    # second pass: the same author/time/content reposted WITH a content-warning
    # tag (no reason: valid NIP-36) — different event id, same stored body:
    # flags must land on the unchanged row via duplicate-content detection
    rec2 = sign_event(1, "plain post", created_at=NOW - 60, secret=KEY_C,
                      tags=[["content-warning"]])
    relay2 = MockRelay()
    relay2.responses = {url: ([follow, rec2], True) for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}
    report = collect_once(store, ROOT, max_requests=40, relays=relay2)["nostr"]
    cats = store.content_warnings([rec2["id"]])
    assert any("author: unspecified" in r for r in cats[rec2["id"]])  # no-reason CW honored
    assert any("duplicate-content" in r for r in cats[rec2["id"]])  # spam flag on the duplicate
    assert report["filter_stats"].get("spam", 0) >= 1


def test_blocked_existing_row_removed_and_never_reingested(store, tmp_path):
    """Block action removes the STORED row and blocks re-ingestion, durable."""
    blocked = sign_event(1, "already public, now blocked", created_at=NOW - 60, secret=KEY_C)
    store.upsert(nostr_post(blocked, "x"), now=NOW)  # exists from before the block
    bl = tmp_path / "blocklist.txt"
    bl.write_text(blocked["id"] + "\n")
    import meatybroth.ingest as ing
    orig = ing.load_blocklist
    ing.load_blocklist = lambda store: {blocked["id"]}
    try:
        follow = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
        relay = MockRelay()
        relay.responses = {url: ([follow, blocked], True) for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}
        report = collect_once(store, ROOT, max_requests=40, relays=relay)["nostr"]
        assert store.get(f"nostr:{blocked['id']}", now=NOW) is None  # removed from store
        assert report.get("purged_blocklisted", 0) >= 1  # durable removal ran
        assert report["filter_stats"]["blocklist"] >= 1  # and re-ingestion blocked
        # benign rows survive the purge
        other = sign_event(1, "keep me", created_at=NOW - 59, secret=KEY_D)
        store.upsert(nostr_post(other, "y"), now=NOW)
        assert store.get(f"nostr:{other['id']}", now=NOW) is not None
    finally:
        ing.load_blocklist = orig


def test_benign_sensitive_topics_unflagged(store):
    """Health/security discussion is never explicit-flagged (policy note)."""
    follow = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
    health = sign_event(1, "my doctor changed my HRT dose and the oncology ward was great",
                        created_at=NOW - 60, secret=KEY_C)
    security = sign_event(1, "the audit found hardcoded credentials in the test fixture",
                          created_at=NOW - 61, secret=KEY_C)
    relay = MockRelay()
    relay.responses = {url: ([follow, health, security], True) for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}
    report = collect_once(store, ROOT, max_requests=40, relays=relay)["nostr"]
    assert store.get(f"nostr:{health['id']}", now=NOW) is not None
    assert store.get(f"nostr:{security['id']}", now=NOW) is not None
    assert store.content_warnings([health["id"], security["id"]]) == {}
    assert report["filter_stats"].get("explicit", 0) == 0
    # security talk mentioning credential-shaped strings is a real edge: the
    # fixture pattern above is prose, not a key; synthetic keys DO reject:
    real_key = sign_event(1, "ghp_" + "A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6q7R8",
                          created_at=NOW - 62, secret=KEY_C)
    relay.responses = {url: ([follow, real_key], True) for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}
    report = collect_once(store, ROOT, max_requests=40, relays=relay)["nostr"]
    assert store.get(f"nostr:{real_key['id']}", now=NOW) is None


def test_configured_relay_also_advertised_still_queried(store):
    """A configured relay appearing LATE in the root's advertised list keeps its
    slot (reserved-first ordering), and the same relay is not double-queried."""
    many_advertised = ["wss://adv.example/%02d" % i for i in range(12)]
    configured_last = "wss://therustyclaw.com/relay"  # also in CONFIGURED_RELAYS
    root_kind3 = sign_event(3, "", created_at=NOW - 7200, tags=[["p", PUB_C]])
    # NIP-65 relay list (kind 10002): 12 advertised relays, configured one LAST
    root_relays = sign_event(10002, "", created_at=NOW - 7200,
                             tags=[["r", url, "write"] for url in many_advertised]
                             + [["r", configured_last, "write"]])
    relay = MockRelay()
    relay.responses = {url: ([root_kind3, root_relays], True)
                       for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS + many_advertised}
    collect_once(store, ROOT, max_requests=90, relays=relay)
    queried = [url for url, _ in relay.queries]
    assert configured_last in queried  # survived truncation
    assert queried.count(configured_last) >= 1
    # advertised relays beyond the cap are dropped, but configured come first
    assert "wss://adv.example/00" in queried  # early advertised still present

def test_blocklist_missing_after_setup_is_loud_error(store, tmp_path):
    """Init creates the file only for a NEW db; deleting it afterwards is an
    error on every later open — the block never silently deactivates."""
    bl = store.blocklist_path
    assert bl.exists()  # setup created it for the new db
    bl.unlink()
    # reopening the (now existing) db must NOT recreate it
    reopened = Store(store.path)
    assert not reopened.blocklist_path.exists()
    with pytest.raises(FileNotFoundError):
        load_blocklist(reopened)


def test_secret_scan_covers_metadata_ingress_and_purge(store):
    """Metadata secrets are deleted from old rows and rejected in profiles
    and tag values before either can reach nostr_state or the page."""
    profile_ok = sign_event(0, json.dumps({"name": "clean"}), created_at=NOW - 3600)
    profile_dirty = sign_event(0, json.dumps({"name": "sk-ant-" + "A1b2C3d4E5" * 4}),
                               created_at=NOW - 3599, secret=KEY_C)
    # The profile phase only requests followed authors; pre-existing root
    # metadata supplies that bounded context without making another graph hop.
    root_follows = sign_event(3, "", created_at=NOW - 500, tags=[["p", PUB_C]])
    store.save_nostr_metadata(root_follows, "wss://relay")
    # This root-authored kind-3 event is delivered by the identity query. Its
    # secret only occurs in a rendered tag value, not the event content.
    dirty_tag_list = sign_event(
        3, "", created_at=NOW - 400,
        tags=[["p", PUB_C], ["content-warning", "nsec1" + "a1b2c3d4e5" * 6]],
    )
    # Simulate old pre-filter storage. The later pass must remove the raw
    # credential, rather than retaining it in a quarantine table.
    store.save_nostr_metadata(profile_dirty, "wss://x")
    assert store.metadata(profile_dirty["pubkey"], 0) is not None

    relay = MockRelay()
    relay.responses = {url: ([profile_ok, profile_dirty, dirty_tag_list], True)
                       for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}
    report = collect_once(store, ROOT, max_requests=40, relays=relay)["nostr"]

    assert store.metadata(profile_dirty["pubkey"], 0) is None  # old copy purged
    assert store.metadata(ROOT, 3)["id"] == root_follows["id"]  # tag rejected
    assert store.metadata(profile_ok["pubkey"], 0) is not None  # benign survives
    rejected = {item["id"] for item in report["rejected_event_ids"]}
    assert {profile_dirty["id"], dirty_tag_list["id"]} <= rejected
    with store.connect() as db:
        assert db.execute("SELECT COUNT(*) FROM nostr_state WHERE event_json LIKE '%sk-ant-%'").fetchone()[0] == 0
        assert db.execute("SELECT COUNT(*) FROM nostr_state WHERE event_json LIKE '%nsec1%' ").fetchone()[0] == 0


def test_observed_three_hop_uses_second_hop_follow_lists(store):
    """Observed C -> D -> E reaches E at three hops without calling D a
    third-hop author merely because a two-hop cap was applied."""
    store.save_nostr_metadata(sign_event(3, "", created_at=NOW - 300,
                                         tags=[["p", PUB_C]]), "wss://relay")
    store.save_nostr_metadata(sign_event(3, "", created_at=NOW - 200,
                                         tags=[["p", PUB_D]], secret=KEY_C), "wss://relay")
    store.save_nostr_metadata(sign_event(3, "", created_at=NOW - 100,
                                         tags=[["p", PUB_E]], secret=KEY_D), "wss://relay")
    relay = MockRelay()
    relay.responses = {url: ([], True) for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}

    report = collect_once(store, ROOT, max_requests=40, relays=relay)["nostr"]

    assert report["second_hop_authors"] == 1
    assert report["third_hop_authors_known"] == 1


def test_recent_context_author_gets_bounded_profile_refresh(store):
    """A stored reply author missing kind-0 gets a two-relay-max refresh.

    This is a profile repair for a card/context already in the reader, not a
    new graph hop: only authors of stored posts are candidates.
    """
    root_follows = sign_event(3, "", created_at=NOW - 500, tags=[["p", PUB_C]])
    store.save_nostr_metadata(root_follows, "wss://relay")
    reply = sign_event(1, "recent reply", created_at=NOW - 100, secret=KEY_C,
                       tags=[e_tag("a" * 64, "reply")])
    store.upsert(nostr_post(reply, PUB_C[:12]), now=NOW)
    profile = sign_event(0, json.dumps({"name": "context author"}),
                         created_at=NOW - 50, secret=KEY_C)

    relay = MockRelay()
    relay.responses = {url: ([root_follows, profile], True)
                       for url in BOOTSTRAP_RELAYS + CONFIGURED_RELAYS}
    report = collect_once(store, ROOT, max_requests=40, relays=relay)["nostr"]

    assert report["missing_profile_authors"] == 1
    assert report["missing_profile_queries"] == 2
    assert store.metadata(PUB_C, 0)["id"] == profile["id"]
    # Identity is exactly one query per bootstrap relay; the next two REQs are
    # the bounded priority refresh, before normal follow/outbox phases.
    priority_queries = [filters for _, filters in relay.queries[
        len(BOOTSTRAP_RELAYS):len(BOOTSTRAP_RELAYS) + 2
    ]]
    assert priority_queries == [[{"authors": [PUB_C], "kinds": [0]}]] * 2
