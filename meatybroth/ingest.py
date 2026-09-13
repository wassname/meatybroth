"""Bounded Nostr collection for Meaty Broth (bridges included; all content is Nostr).

Every relay REQ and parent fetch spends one request from the shared budget.
Gaps (budget stops, missing parents, old bodies, truncated pages, outbox-relay
discovery limits) are reported through store.set_status, never silently
dropped.
"""

from __future__ import annotations

from collections import Counter
from dataclasses import dataclass
from datetime import datetime, timezone
import hashlib
import json
import re
import time

from coincurve import PublicKeyXOnly
from loguru import logger
from pathlib import Path
import websocket

from meatybroth.store import Post, Store, WINDOW_SECONDS

ROOT_PUBKEY = "60c052cf19fbfb973c1779585df423e3982a3a251fc826d4c76f8063621c5bb6"
BOOTSTRAP_RELAYS = [
    "wss://purplepag.es",
    "wss://nos.lol",
    "wss://relay.damus.io",
    "wss://relay.primal.net",
]
# Configured content relays:
# Mostr carries Bridgy Fed content ONLY for Bluesky users who opted in —
# never present it as complete Bluesky coverage. Momostr is the fediverse
# bridge; The Rusty Claw is the public agent relay.
CONFIGURED_RELAYS = [
    "wss://relay.mostr.pub",
    "wss://relay.momostr.pink",
    "wss://therustyclaw.com/relay",
]
IDENTITY_KINDS = (0, 3, 10002)
RELAY_TIMEOUT = 12.0
MAX_QUERY_BYTES = 500_000
MAX_TEXT = 64 * 1024
SECOND_HOP_CAP = 50
THIRD_HOP_CAP = 100  # bounded observed-3hop post collection
CONTENT_RELAYS_TOTAL_CAP = 10  # initial 4 + bounded followed-author outbox relays
BASE_PHASE_SHARE = 0.5  # of the post-wave remainder; backfill/discovery/parents share the rest
POST_PAGE_LIMIT = 500
BACKFILL_PAGES_PER_RELAY = 2  # a fat relay must not starve follow-lists/discovery
MISSING_PROFILE_CAP = 25  # recent/context authors per bounded kind-0 refresh
MISSING_PROFILE_RELAY_CAP = 2  # probes per pass; normal profile wave still covers follows

PRIMAL_CACHE_URL = "wss://cache2.primal.net/v1"
PRIMAL_LIST_AUTHOR = "5d8282fc89410f1c57681a2c3b8be57afd1566c262fd1deb543999d39d141cb4"
PRIMAL_LISTS = ("spam_list", "nsfw_list")
PRIMAL_LIST_REFRESH_SECONDS = 24 * 3600

PUBKEY_HEX = re.compile(r"[0-9a-f]{64}")
SIG_HEX = re.compile(r"[0-9a-f]{128}")


# ---------------------------------------------------------------- Nostr events


def event_digest(event: dict) -> bytes:
    canonical = [0, event["pubkey"], event["created_at"], event["kind"], event["tags"], event["content"]]
    return hashlib.sha256(json.dumps(canonical, ensure_ascii=False, separators=(",", ":")).encode()).digest()


def verify_event(event: dict) -> bool:
    """Canonical NIP-01 ID hash plus BIP-340 Schnorr check; False on malformed input."""
    try:
        if not PUBKEY_HEX.fullmatch(event["pubkey"]) or not SIG_HEX.fullmatch(event["sig"]):
            return False
        if not PUBKEY_HEX.fullmatch(event["id"]):
            return False
        digest = event_digest(event)
        if digest.hex() != event["id"]:
            return False
        return PublicKeyXOnly(bytes.fromhex(event["pubkey"])).verify(bytes.fromhex(event["sig"]), digest)
    except (KeyError, TypeError, ValueError):
        return False


def utc_time(timestamp: int) -> str:
    return datetime.fromtimestamp(timestamp, timezone.utc).isoformat().replace("+00:00", "Z")


def primal_list_members(event: dict, identifier: str, *, author: str = PRIMAL_LIST_AUTHOR) -> set[str]:
    """Validate one Primal cached NIP-33 list before trusting its p tags."""
    if not verify_event(event):
        raise ValueError("Primal list has invalid Nostr signature")
    if event["pubkey"] != author or event["kind"] != 30000:
        raise ValueError("Primal list has unexpected author or kind")
    if [tag[1] for tag in event["tags"] if len(tag) >= 2 and tag[0] == "d"] != [identifier]:
        raise ValueError("Primal list has unexpected d tag")
    return {tag[1] for tag in event["tags"] if len(tag) >= 2 and tag[0] == "p" and PUBKEY_HEX.fullmatch(tag[1])}


def fetch_primal_list(identifier: str) -> dict:
    """Read one public categorized list from Primal's cache, with a bounded
    request. The cache is not counted as a content relay or a follows query."""
    subscription = f"meatybroth-{identifier}"
    ws = websocket.create_connection(PRIMAL_CACHE_URL, timeout=RELAY_TIMEOUT)
    try:
        ws.send(json.dumps(["REQ", subscription, {"cache": ["parameterized_replaceable_list", {
            "pubkey": PRIMAL_LIST_AUTHOR, "identifier": identifier}]}]))
        deadline = time.monotonic() + RELAY_TIMEOUT
        while time.monotonic() < deadline:
            ws.settimeout(max(0.1, deadline - time.monotonic()))
            raw = ws.recv()
            if len(raw.encode()) > MAX_QUERY_BYTES:
                raise ValueError(f"Primal cache response exceeds {MAX_QUERY_BYTES} bytes")
            message = json.loads(raw)
            if message[:2] == ["EVENT", subscription]:
                event = message[2]
                # Validate before returning; cache auxiliary events are ignored.
                primal_list_members(event, identifier)
                return event
            if message[:2] == ["EOSE", subscription]:
                break
    finally:
        ws.close()
    raise RuntimeError(f"Primal cache returned no {identifier} event")


def refresh_primal_lists(store: Store, *, now: int) -> dict:
    """At most daily, check each Primal list. A failed attempt is also held
    for a day; a later-created, non-future event alone may replace a snapshot."""
    detail = {"cache": PRIMAL_CACHE_URL, "author": PRIMAL_LIST_AUTHOR, "lists": {}}
    for identifier in PRIMAL_LISTS:
        cached = store.moderation_list("primal", identifier)
        attempt = store.moderation_refresh_attempt("primal", identifier)
        if attempt is not None and now - attempt["attempted_at"] < PRIMAL_LIST_REFRESH_SECONDS:
            detail["lists"][identifier] = {"refresh": "not-due", "attempted_at": attempt["attempted_at"],
                "last_error": attempt["error"]}
            if cached is not None:
                detail["lists"][identifier]["using_cached"] = {"event_id": cached["event_id"],
                    "event_created_at": cached["event_created_at"],
                    "event_created_at_utc": utc_time(cached["event_created_at"]),
                    "checked_at": cached["checked_at"],
                    "members": len(cached["members"])}
            continue
        try:
            event = fetch_primal_list(identifier)
            members = primal_list_members(event, identifier)
            if event["created_at"] > now:
                raise ValueError("Primal list event is future-dated")
            if cached is not None and event["created_at"] < cached["event_created_at"]:
                raise ValueError("Primal list event rolls back verified snapshot")
            previous_members = set() if cached is None else cached["members"]
            store.set_moderation_list("primal", identifier, event_id=event["id"], author=event["pubkey"],
                                      event_created_at=event["created_at"], members=members, checked_at=now)
            store.record_moderation_refresh_attempt("primal", identifier, attempted_at=now, error=None)
            reconciled = store.reconcile_primal_list(identifier, members, now=now)
            detail["lists"][identifier] = {"event_id": event["id"],
                "event_created_at": event["created_at"],
                "event_created_at_utc": utc_time(event["created_at"]),
                "checked_at": now,
                "members": len(members), "reconciled_posts": reconciled,
                "resync_required_authors": len(previous_members - members) if identifier == "nsfw_list" else 0,
                "refresh": "verified"}
        except (OSError, websocket.WebSocketException, json.JSONDecodeError, ValueError, RuntimeError) as error:
            store.record_moderation_refresh_attempt("primal", identifier, attempted_at=now,
                                                    error=f"{type(error).__name__}: {error}")
            detail["lists"][identifier] = {"refresh": "error", "error": f"{type(error).__name__}: {error}"}
            if cached is not None:
                detail["lists"][identifier]["using_cached"] = {"event_id": cached["event_id"],
                    "event_created_at": cached["event_created_at"],
                    "event_created_at_utc": utc_time(cached["event_created_at"]),
                    "checked_at": cached["checked_at"],
                    "members": len(cached["members"])}
    store.set_status("primal-moderation", detail, now=now)
    return detail


def thread_refs(event: dict) -> tuple[str | None, str | None]:
    """NIP-10 (root, reply) hex ids; marked tags win, else first=root, last=reply."""
    marked_root = marked_reply = None
    unmarked: list[str] = []
    for tag in event.get("tags", []):
        if isinstance(tag, list) and len(tag) >= 2 and tag[0] == "e" and PUBKEY_HEX.fullmatch(tag[1]):
            marker = tag[3] if len(tag) >= 4 else None
            if marker == "root" and marked_root is None:
                marked_root = tag[1]
            elif marker == "reply" and marked_reply is None:
                marked_reply = tag[1]
            elif marker is None:
                unmarked.append(tag[1])
    root = marked_root or (unmarked[0] if unmarked else None)
    reply = marked_reply or (unmarked[-1] if unmarked else None)
    if root is not None and reply is None:
        # NIP-10: "For top level replies (those replying directly to the root
        # event), only the root marker should be used" — the root doubles as reply.
        reply = root
    if root is None and reply is not None:
        root = reply  # NIP-10: a reply tag with no root tag doubles as the root
    return root, reply


def clipped(text: str) -> str:
    if len(text) <= MAX_TEXT:
        return text
    return text[:MAX_TEXT] + "\n\n[truncated]"


def nostr_post(event: dict, author_name: str) -> Post:
    root, reply = thread_refs(event)
    return Post(
        source="nostr",
        source_id=event["id"],
        author_id=event["pubkey"],
        author_name=author_name,
        text=clipped(event["content"]),
        created_at=event["created_at"],
        url=f"https://njump.me/{event['id']}",
        parent_id=f"nostr:{reply}" if reply else None,
        root_id=f"nostr:{root}" if root else None,
    )


def author_name(store: Store, pubkey: str) -> str:
    event = store.metadata(pubkey, 0)
    if event:
        try:
            profile = json.loads(event.get("content", ""))
            name = profile.get("display_name") or profile.get("name")
            if name:
                return str(name)
        except (ValueError, AttributeError):
            pass
    return pubkey[:12]


# ------------------------------------------------------------------- transports


def matches_filters(event: dict, filters: list[dict]) -> bool:
    """NIP-01 client-side check: a relay must only return events that satisfy
    the requested filters; signed-but-unrelated events are not evidence."""
    for f in filters:
        if "ids" in f and event["id"] not in f["ids"]:
            continue
        if "authors" in f and event["pubkey"] not in f["authors"]:
            continue
        if "kinds" in f and event["kind"] not in f["kinds"]:
            continue
        if "since" in f and event["created_at"] < f["since"]:
            continue
        if "until" in f and event["created_at"] > f["until"]:
            continue
        return True
    return False


@dataclass
class QueryResult:
    events: list[dict]
    complete: bool  # True only when the relay sent EOSE before timeout/payload cap


class RelayTransport:
    """One websocket per query, closed at EOSE, timeout or payload cap."""

    def query(self, relay_url: str, filters: list[dict], *, timeout: float = RELAY_TIMEOUT) -> QueryResult:
        ws = websocket.create_connection(relay_url, timeout=timeout, max_size=MAX_QUERY_BYTES)
        try:
            ws.send(json.dumps(["REQ", "meatybroth", *filters]))
            events, received = [], 0
            while received < MAX_QUERY_BYTES:
                frame = ws.recv()
                received += len(frame)
                message = json.loads(frame)
                if message[0] == "EVENT":
                    events.append(message[2])
                elif message[0] == "EOSE":
                    return QueryResult(events, complete=True)
                elif message[0] in ("NOTICE", "CLOSED"):
                    logger.info("relay {} sent {}: {}", relay_url, message[0], message[1] if len(message) > 1 else "")
                    return QueryResult(events, complete=False)
            logger.warning("relay {} hit payload cap; results partial", relay_url)
            return QueryResult(events, complete=False)
        finally:
            ws.close()



class Budget:
    """Counts every network request; spend() returns False when exhausted."""

    def __init__(self, total: int):
        self.total = total
        self.used = 0

    @property
    def remaining(self) -> int:
        return self.total - self.used

    def spend(self) -> bool:
        if self.used >= self.total:
            return False
        self.used += 1
        return True


# ---------------------------------------------------------------- content filters
# Minimal content controls:
# secrets reject before persistence; spam counts; NIP-36/explicit flags render-side.
# All synthetic-test only; counts land in the collector report, never deleted silently.

SECRET_PATTERNS = [
    ("nsec key", re.compile(r"nsec1[0-9a-z]{30,}")),
    ("pem private key", re.compile(r"-----BEGIN[A-Z ]*PRIVATE KEY-----")),
    ("aws access key", re.compile(r"AKIA[0-9A-Z]{16}")),
    ("anthropic key", re.compile(r"sk-ant-[A-Za-z0-9_-]{20,}")),
    ("openai-style key", re.compile(r"\bsk-[A-Za-z0-9]{20,}\b")),
    ("github token", re.compile(r"gh[pousr]_[A-Za-z0-9]{30,}\b")),
]
URL_RE = re.compile(r"https?://\S+")
EXPLICIT_TERMS = re.compile(
    r"\b(hardcore sex|uncensored nudity|explicit sex scene)\b", re.I)


def find_secret(text: str) -> str | None:
    for name, pattern in SECRET_PATTERNS:
        if pattern.search(text):
            return name
    return None


def blocklist_path(store: Store) -> Path:
    """Anchor to the DB's directory: the container mounts the persistent
    volume there (/var/lib/meatybroth); env overrides for exotic setups."""
    import os
    env = os.environ.get("MEATYBROTH_BLOCKLIST")
    if env:
        return Path(env)
    return store.blocklist_path


def load_blocklist(store: Store) -> set[str]:
    """Operator-maintained pubkeys/event ids, one per line; # comments allowed.

    A missing file is an ERROR for a public deployment (silently disabling the
    block is not acceptable): setup creates an intentional empty file."""
    path = blocklist_path(store)
    if not path.exists():
        logger.error("blocklist file {} MISSING: create it (even empty) before collecting", path)
        raise FileNotFoundError(f"blocklist file missing: {path}")
    return {line.strip() for line in path.read_text().splitlines()
            if line.strip() and not line.startswith("#")}


def spam_reasons(event: dict, store: Store) -> list[str]:
    """Cheap spam flags: same-author duplicate content in-window, link-farm."""
    reasons = []
    urls = URL_RE.findall(event["content"])
    words = len(event["content"].split())
    if len(urls) > 5 and words < 40:
        reasons.append("link-farm")
    with store.connect() as db:
        dup = db.execute(
            "SELECT COUNT(*) FROM posts WHERE author_id=? AND text=? AND canonical_id != ?",
            (event["pubkey"], event["content"], f"nostr:{event['id']}")).fetchone()[0]
    if dup:
        reasons.append("duplicate-content")
    return reasons


# --------------------------------------------------------------------- Nostr


def collect_nostr(store: Store, root_pubkey: str, relays, budget: Budget, *, now: int, cap: int) -> dict:
    report: dict = {
        "requests_used": 0,
        "relays_queried": [],
        "relay_errors": {},
        "follows": 0,
        "posts_upserted": 0,
        "posts_unchanged": 0,
        "invalid_events": 0,
        "metadata_saved": 0,
        "metadata_unchanged": 0,
        "missing_profile_authors": 0,
        "missing_profile_queries": 0,
        "second_hop_authors": 0,
        "second_hop_capped": False,
        "parent_fetches": 0,
        "parents_missing": 0,
        "parents_older_than_window": 0,
        "backfill_pages": 0,
        "filter_mismatched": 0,
        "filter_stats": Counter(),
        "follow_outbox_relays_added": 0,
        "partial": False,
        "gaps": [],
    }
    start_used = budget.used
    blocklist = load_blocklist(store)
    primal_spam = (store.moderation_list("primal", "spam_list") or {"members": set()})["members"]
    primal_nsfw = (store.moderation_list("primal", "nsfw_list") or {"members": set()})["members"]

    def fetch(relay: str, filters: list[dict]) -> QueryResult | None:
        if budget.used >= cap or not budget.spend():
            return None
        if relay not in report["relays_queried"]:
            report["relays_queried"].append(relay)
        try:
            return relays.query(relay, filters, timeout=RELAY_TIMEOUT)
        except Exception as error:  # one bad relay must not stop the others
            report["relay_errors"][relay] = f"{type(error).__name__}: {error}"
            return None

    def accept(result: QueryResult | None, relay: str, *, posts: bool,
               filters: list[dict] | None = None) -> list[dict]:
        """Verify, filter-match and persist; returns verified matching events."""
        if result is None:
            return []
        if not result.complete:
            report["partial"] = True
            report["gaps"].append(f"{relay}: query ended before EOSE")
        kept = []
        for event in result.events:
            if not verify_event(event):
                report["invalid_events"] += 1
                continue
            if filters is not None and not matches_filters(event, filters):
                # Signed but unrelated: not evidence for this request.
                report["filter_mismatched"] += 1
                continue
            scanned = event["content"]
            if event["kind"] in IDENTITY_KINDS or event["kind"] == 1:
                scanned += " " + " ".join(
                    tag[1] + " " + (tag[2] if len(tag) > 2 else "")
                    for tag in event.get("tags", []) if len(tag) >= 2)
            secret = find_secret(scanned)
            if secret:
                # Reject/quarantine BEFORE persistence — a credential in a
                # profile name or CW reason renders on every card otherwise.
                # Raw signed event is NOT rewritten; it is just never stored.
                if event["id"] not in report.setdefault("_rejected_ids", set()):
                    report["_rejected_ids"].add(event["id"])
                    report["filter_stats"]["secret"] += 1
                    report.setdefault("rejected_event_ids", []).append(
                        {"id": event["id"], "kind": event["kind"], "rule": f"secret:{secret}"})
                logger.info("rejected {} {}: secret pattern {}", event["kind"], event["id"][:16], secret)
                continue
            if posts and event["kind"] == 1:
                if event["pubkey"] in blocklist or event["id"] in blocklist:
                    if event["id"] not in report.setdefault("_rejected_ids", set()):
                        report["_rejected_ids"].add(event["id"])
                        report["filter_stats"]["blocklist"] += 1
                        report.setdefault("rejected_event_ids", []).append(
                            {"id": event["id"], "rule": "blocklist"})
                    logger.info("rejected event {}: blocklist", event["id"][:16])
                    continue  # always skip: seen-tracking prevents double COUNT, not double skip
            if event["kind"] == 1:
                if posts:
                    spam = spam_reasons(event, store)
                    primal_spam_member = event["pubkey"] in primal_spam
                    seen_spam = report.setdefault("_spam_ids", set())
                    for reason in spam:
                        if event["id"] not in seen_spam:  # one relay copy counts once
                            seen_spam.add(event["id"])
                            report["filter_stats"]["spam"] += 1
                    if primal_spam_member and event["id"] not in seen_spam:
                        seen_spam.add(event["id"])
                        report["filter_stats"]["spam"] += 1
                    cw_reason = next((t[1] if len(t) >= 2 and t[1] else "unspecified"
                                      for t in event.get("tags", []) if t[0] == "content-warning"), None)
                    explicit = bool(EXPLICIT_TERMS.search(event["content"]))
                    if event["pubkey"] in primal_nsfw:
                        if event["id"] not in report.setdefault("_primal_nsfw_ids", set()):
                            report["_primal_nsfw_ids"].add(event["id"])
                            report["filter_stats"]["primal_nsfw"] += 1
                        continue
                    # flags refresh on every sighting, independent of the body
                    # upsert: preexisting unchanged posts acquire tags too
                    if cw_reason is not None:
                        store.save_content_warning(event["id"], "author-cw", f"author: {cw_reason}", now=now)
                    if explicit:
                        store.save_content_warning(event["id"], "explicit", "auto-flagged: explicit", now=now)
                    if event["pubkey"] in primal_nsfw:
                        store.save_content_warning(event["id"], "primal-nsfw",
                                                   "curated-nsfw: Primal snapshot", now=now)
                    for reason in spam:
                        store.save_content_warning(event["id"], "spam", f"auto-flagged: spam {reason}", now=now)
                    if primal_spam_member:
                        store.save_content_warning(event["id"], "primal-spam",
                                                   "auto-flagged: spam Primal snapshot", now=now)
                    if cw_reason is not None and event["id"] not in report.setdefault("_cw_ids", set()):
                        report["_cw_ids"].add(event["id"])
                        report["filter_stats"]["cw"] += 1
                    if explicit and event["id"] not in report.setdefault("_explicit_ids", set()):
                        report["_explicit_ids"].add(event["id"])
                        report["filter_stats"]["explicit"] += 1
                    post = nostr_post(event, author_name(store, event["pubkey"]))
                    if store.upsert(post, now=now):
                        report["posts_upserted"] += 1
                    else:
                        report["posts_unchanged"] += 1
            elif event["kind"] in IDENTITY_KINDS:
                if store.save_nostr_metadata(event, relay):
                    report["metadata_saved"] += 1
                else:
                    report["metadata_unchanged"] += 1
            kept.append(event)
        return kept

    # Identity: latest-found kind 0/3/10002 from bootstrap relays, then the
    # root's advertised write relays found in wave one.
    advertised: list[str] = []

    def query_identity(relay: str):
        filters = [{"authors": [root_pubkey], "kinds": [kind], "limit": 1} for kind in IDENTITY_KINDS]
        for event in accept(fetch(relay, filters), relay, posts=False, filters=filters):
            if event["kind"] != 10002:
                continue
            for tag in event["tags"]:
                if len(tag) >= 2 and tag[0] == "r" and tag[1].startswith("wss://"):
                    marker = tag[2] if len(tag) > 2 else None
                    if marker in (None, "", "write") and tag[1].rstrip("/") not in advertised:
                        advertised.append(tag[1].rstrip("/"))

    for relay in BOOTSTRAP_RELAYS:
        query_identity(relay)
    for relay in advertised[:2]:
        query_identity(relay)

    follows = store.followed(root_pubkey)
    report["follows"] = len(follows)
    if not follows:
        report["gaps"].append("no follow list available; content phases skipped")
        report["requests_used"] = budget.used - start_used
        return report

    # CONFIGURED_RELAYS first: their slots are guaranteed; advertised relays
    # and bootstrap fallbacks fill the rest. Deduped, so a configured relay
    # that is also advertised is not double-listed nor truncated away.
    content_relays = list(dict.fromkeys(
        CONFIGURED_RELAYS + advertised + ["wss://purplepag.es", "wss://nos.lol"]))[:CONTENT_RELAYS_TOTAL_CAP]
    collected: list[dict] = []
    stopped = False

    # Cards and context render a profile when it exists. Direct follows get a
    # full profile wave below; spend at most two additional REQs per pass on
    # recent missing names, with reply/context authors first. This deliberately
    # refreshes only authors we already stored, not another graph expansion.
    missing_profiles = store.recent_authors_missing_profiles(MISSING_PROFILE_CAP)
    report["missing_profile_authors"] = len(missing_profiles)
    if missing_profiles:
        profile_filter = {"authors": missing_profiles, "kinds": [0]}
        for relay in content_relays[:MISSING_PROFILE_RELAY_CAP]:
            result = fetch(relay, [profile_filter])
            if result is None:
                break
            accept(result, relay, posts=False, filters=[profile_filter])
            report["missing_profile_queries"] += 1

    # Followed authors' own advertised write relays: bridge accounts (RSS/
    # Mastodon bridges) often publish only there. One bounded REQ per initial
    # relay collects their kind-10002 state; the most-advertised relays join
    # the content set under the total cap, within the same budget.
    for relay in list(content_relays):
        result = fetch(relay, [{"authors": sorted(follows), "kinds": [10002], "limit": 500}])
        if result is None and (budget.used >= cap or budget.remaining == 0):
            report["gaps"].append(f"budget stopped before follow outbox-relay discovery on {relay}")
            break
        accept(result, relay, posts=False, filters=[{"authors": sorted(follows), "kinds": [10002]}])
    advertised_by_follows: Counter = Counter()
    for follower in follows:
        event = store.metadata(follower, 10002)
        if not event:
            continue
        for tag in event.get("tags", []):
            if len(tag) >= 2 and tag[0] == "r" and tag[1].startswith("wss://"):
                marker = tag[2] if len(tag) > 2 else None
                if marker in (None, "", "write"):
                    advertised_by_follows[tag[1].rstrip("/")] += 1
    extra_candidates = [relay for relay, _ in advertised_by_follows.most_common()
                        if relay not in content_relays]
    if extra_candidates:
        # Prefer relays that explain coverage gaps: how many follows with no
        # stored posts advertise this relay (they are the authors we are
        # missing), then overall advertiser count.
        with store.connect() as db:
            marks = ",".join("?" * len(follows))
            posted = {r[0] for r in db.execute(
                f"SELECT DISTINCT author_id FROM posts WHERE author_id IN ({marks})", tuple(follows)).fetchall()}
        missing_authors = follows - posted

        def relay_score(relay: str) -> tuple[int, int, int]:
            # Rank: missing authors for whom this relay is the ONLY advertised
            # write relay (sole path to their posts), then any missing
            # advertisers, then overall advertiser count.
            sole = any_missing = 0
            for f in missing_authors:
                relays_f = {t[1].rstrip("/") for t in
                            (store.metadata(f, 10002) or {}).get("tags", [])
                            if len(t) >= 2 and t[0] == "r"
                            and (len(t) < 3 or t[2] in (None, "", "write"))}
                if not relays_f:
                    continue
                if relay in relays_f:
                    any_missing += 1
                    if relays_f == {relay}:
                        sole += 1
            return (sole, any_missing, advertised_by_follows[relay])

        extra_candidates.sort(key=relay_score, reverse=True)
    # reserve the configured-relay slots: extras only fill the remainder
    extra = extra_candidates[:max(0, CONTENT_RELAYS_TOTAL_CAP - len(content_relays))]
    if extra:
        report["follow_outbox_relays_added"] = len(extra[:CONTENT_RELAYS_TOTAL_CAP - len(content_relays)])
        content_relays += extra[:CONTENT_RELAYS_TOTAL_CAP - len(content_relays)]

    # Base phases for EVERY relay first (so newly discovered outbox relays are
    # not starved by earlier relays' backfills), then bounded backfills with a
    # GUARANTEED share: without one, base phases spend the whole budget and
    # every pass re-fetches the same newest pages, never advancing history.
    # Computed after identity+outbox wave; a floor of 3 keeps the first relay's
    # posts phase reachable even on tiny budgets.
    base_cap = budget.used + max(3, int((cap - budget.used) * BASE_PHASE_SHARE))  # ~50%
    backfill_jobs: list[tuple[str, dict]] = []
    for relay in content_relays:
        for filters, phase in (
            ({"authors": sorted(follows), "kinds": [0]}, "profiles"),
            ({"authors": sorted(follows), "kinds": [1], "since": now - WINDOW_SECONDS, "limit": 500}, "posts"),
            ({"authors": sorted(follows), "kinds": [3]}, "follow-lists"),
        ):
            result = fetch(relay, [filters]) if budget.used < base_cap else None
            if result is None and (budget.used >= base_cap or budget.remaining == 0):
                if phase == "posts":
                    report["gaps"].append(f"budget base-share spent before {phase} on {relay}")
                stopped = phase == "posts"  # profiles/follow-lists yield to keep posts flowing
                break
            accepted = [e for e in accept(result, relay, posts=True, filters=[filters]) if e["kind"] == 1] \
                if result is not None else []
            collected += accepted
            # A full page or an early transport end (raw transport fullness)
            # never proves older history is exhausted. The cursor state uses
            # ACCEPTED events only (verified + filter-matched): a malformed or
            # unrelated page cannot crash the cursor or steer it off-thread.
            if phase == "posts" and accepted:
                backfill_jobs.append((relay, filters, {e["id"] for e in accepted},
                                      accepted, result.complete))
        if stopped:
            break

    # The guaranteed backfill share runs even when base phases hit their cap
    # (that is the normal case, not a failure) — only a dead budget skips it.
    # Cross-run cursors: each relay continues from its PERSISTED oldest
    # position, so successive passes dig deeper instead of re-reading the
    # same newest pages. First sighting falls back to the base-phase page.
    backfill_cap = budget.used + max(2, int((cap - budget.used) * 0.5))
    if budget.remaining > 0:
        for relay, filters, seen_ids, accepted, page_complete in backfill_jobs:
            saved = store.get_cursor(relay)
            if saved is not None:
                if saved <= now - WINDOW_SECONDS:
                    continue  # already walked this relay to the window floor
                cursor_filter = {**filters, "until": saved}
                result = fetch(relay, [cursor_filter])
                if result is None:
                    continue
                page_complete = result.complete
                accepted = [e for e in accept(result, relay, posts=True, filters=[cursor_filter]) if e["kind"] == 1]
                collected += accepted
            backfills = 0
            while accepted and \
                    (len(accepted) >= POST_PAGE_LIMIT or not page_complete):
                if backfills >= BACKFILL_PAGES_PER_RELAY:
                    report["gaps"].append(f"{relay}: backfill stopped at {backfills} pages this run")
                    break
                if budget.used >= backfill_cap:
                    report["gaps"].append(f"{relay}: backfill budget share spent")
                    break
                oldest = min(e["created_at"] for e in accepted)
                store.save_cursor(relay, oldest)  # persisted: next pass continues here
                backfill_filter = {**filters, "until": oldest}
                result = fetch(relay, [backfill_filter])
                if result is None:
                    if budget.used >= cap or budget.remaining == 0:
                        report["gaps"].append(f"budget stopped before older posts on {relay}")
                    break
                page_complete = result.complete
                accepted = [e for e in accept(result, relay, posts=True, filters=[backfill_filter]) if e["kind"] == 1]
                collected += accepted
                report["backfill_pages"] += 1
                backfills += 1
                new_ids = {e["id"] for e in accepted} - seen_ids
                seen_ids |= new_ids
                if not new_ids:
                    report["gaps"].append(
                        f"{relay}: backfill stalled, no unseen events at or below created_at {oldest}")
                    break

    if not stopped:
        # Two-hop discovery: rank discovered authors by direct-follower count.
        follower_counts: Counter[str] = Counter()
        for follower in follows:
            for discovered in store.followed(follower):
                if discovered not in follows and discovered != root_pubkey:
                    follower_counts[discovered] += 1
        second_hop = [key for key, _ in follower_counts.most_common(SECOND_HOP_CAP)]
        report["second_hop_authors"] = len(second_hop)
        report["second_hop_capped"] = len(follower_counts) > len(second_hop)
        if not second_hop:
            report["gaps"].append("no second-hop authors discovered; discovery queries skipped")
            second_hop = None
        # Third hop means follows of OBSERVED second-hop accounts. Exclude all
        # direct-follows' follows, even those below the top-50 query cap: a
        # capped two-hop author must not be relabelled as three-hop.
        all_two_hop = set(follower_counts)
        third_hop = sorted({c for second in (second_hop or []) for c in store.followed(second)}
                           - all_two_hop - follows - {root_pubkey})
        third_hop = [c for c in third_hop
                     if any(store.metadata(second, 3) for second in (second_hop or [])
                            if c in store.followed(second))]
        report["third_hop_authors_known"] = len(third_hop)
        if len(third_hop) > THIRD_HOP_CAP:
            report["gaps"].append(f"third-hop authors truncated to {THIRD_HOP_CAP} of {len(third_hop)}")
            third_hop = third_hop[:THIRD_HOP_CAP]
        if second_hop:
            for relay in content_relays:
                for hop_filter, phase in (
                    ({"authors": sorted(second_hop + third_hop), "kinds": [1], "since": now - WINDOW_SECONDS, "limit": 500}, "discovery posts"),
                    ({"authors": sorted(second_hop), "kinds": [3]}, "discovery follow-lists"),
                ):
                    result = fetch(relay, [hop_filter])
                    if result is None and (budget.used >= cap or budget.remaining == 0):
                        report["gaps"].append(f"budget stopped before {phase} on {relay}")
                        break
                    collected += [e for e in accept(result, relay, posts=True, filters=[hop_filter]) if e["kind"] == 1]

    resolve_parents(store, content_relays, fetch, accept, report, collected, budget=budget, cap=cap, now=now)
    report["requests_used"] = budget.used - start_used
    return report


def resolve_parents(store, content_relays, fetch, accept, report, collected, *, budget: Budget, cap: int, now: int) -> None:
    """Exact-ID fetches for missing parents; recent orphans stay, old bodies don't."""
    missing: list[str] = []
    for event in collected:
        _, reply = thread_refs(event)
        if reply and store.get(f"nostr:{reply}", now=now) is None and reply not in missing:
            missing.append(reply)
    if not missing:
        return
    if not content_relays:
        report["parents_missing"] = len(missing)
        report["gaps"].append(f"{len(missing)} parents not fetched: no relay available")
        return
    for parent_id in missing:
        found = []
        used_relay = None
        for relay in content_relays:  # try each until one serves the exact id
            result = fetch(relay, [{"ids": [parent_id]}])
            if result is None:
                if budget.used >= cap or budget.remaining == 0:
                    break
                continue
            found = [e for e in result.events if verify_event(e) and e["id"] == parent_id]
            if found:
                used_relay = relay
                break
        if not found:
            if budget.used >= cap or budget.remaining == 0:
                report["gaps"].append(f"{len(missing)} parent fetches deferred by budget")
                break
            report["parents_missing"] += 1
            continue
        report["parent_fetches"] += 1
        parent = found[0]
        if not now - WINDOW_SECONDS <= parent["created_at"] <= now:
            # Keep the recent reply's dangling pointer; never the old body.
            report["parents_older_than_window"] += 1
            continue
        accept(QueryResult([parent], complete=True), used_relay, posts=True,
               filters=[{"ids": [parent_id]}])


def collect_once(store: Store, root_pubkey: str, *, max_requests: int = 40, relays=None,
                 refresh_primal: bool = True) -> dict:
    """One bounded Nostr collection pass; returns the per-pass report.
    Tests with an injected relay transport can set refresh_primal=False to keep
    synthetic tests independent of Primal's public cache."""
    relays = relays if relays is not None else RelayTransport()
    budget = Budget(max_requests)
    now = int(time.time())
    primal = refresh_primal_lists(store, now=now) if refresh_primal else {"refresh": "disabled by caller"}

    nostr = collect_nostr(store, root_pubkey, relays, budget, now=now, cap=max_requests)
    nostr["primal_moderation"] = primal
    # durable operator block action: remove stored content for blocked
    # ids/authors so it never renders again, not just block new ingress
    nostr["purged_blocklisted"] = store.purge_blocklisted(load_blocklist(store))
    nostr["metadata_secret_purged"] = store.purge_metadata_with_secrets(find_secret)
    # purge already-stored copies carrying secrets (backfill scan)
    with store.connect() as db:
        for canonical_id, text in db.execute("SELECT canonical_id, text FROM posts").fetchall():
            secret = find_secret(text)
            if secret:
                db.execute("DELETE FROM posts WHERE canonical_id=?", (canonical_id,))
                nostr["filter_stats"]["secret_purged"] += 1
                logger.info("purged stored event {}: secret pattern {}", canonical_id[:16], secret)
    for internal in ("_rejected_ids", "_spam_ids", "_cw_ids", "_explicit_ids", "_primal_nsfw_ids"):
        nostr.pop(internal, None)  # internal uniqueness tracking
    nostr["filter_stats"] = dict(nostr["filter_stats"])  # JSON-serializable for status
    store.set_status("nostr", nostr, now=now)
    expired = store.expire(now=now)
    return {
        "max_requests": max_requests,
        "requests_used": budget.used,
        "expired_rows": expired,
        "nostr": nostr,
    }
