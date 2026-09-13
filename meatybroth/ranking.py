"""Feed and search orderings over the shared Store schema.

Four modes with genuinely different orderings:
new (recency), relevance (FTS5 BM25 ascending), conversations (thread cards by
distinct recent reply authors), social discovery (Nostr follows-of-follows only).

Every query applies the strict 30-day creation-time window; the Nostr graph
restriction applies to social discovery alone.
"""

import math
from collections import Counter
import re

from meatybroth.store import Store, WINDOW_SECONDS, timestamp

PAGE_SIZE = 50
MODES = ("new", "relevance", "conversations", "discovery")
MODE_EXPLANATIONS = {
    "new": "Every post from the last 30 days, newest first.",
    "relevance": "Posts matching your terms, best match first (BM25: lower = closer); newer breaks ties.",
    "conversations": "Threads with the most distinct recent repliers first; with a query, only matching threads.",
    "discovery": "Recent posts within the selected 1–3 hop reach. Connections favours authors supported by more independent accounts you follow; lower distance score ranks first. Based on the collected, incomplete follow graph.",
}

MODE_LABELS = {
    "new": "New",
    "relevance": "Relevance",
    "conversations": "Conversations",
    "discovery": "Social",
}
DAY = 86400

# Terms are unicode word runs; anything else must arrive inside quotes or is dropped.
TERM = re.compile(r"\w+", re.UNICODE)


class QueryError(ValueError):
    """A user-visible search-parse failure, never an empty successful result."""


def parse_fts(query: str) -> str:
    """User query -> FTS5 MATCH expression with AND-ed terms and quoted phrases.

    Raises QueryError on unbalanced quotes or a query with no searchable text,
    so the page can show an error instead of silent empty results.
    """
    if query.count('"') % 2:
        raise QueryError('Unmatched quotation mark in search query.')
    parts = query.split('"')
    phrases = [p for p in parts[1::2] if TERM.search(p)]
    terms = [m.group() for p in parts[0::2] for m in TERM.finditer(p)]
    # Quote bare terms too: FTS5 treats uppercase AND/OR/NOT as operators,
    # which would 500 instead of matching the literal word.
    clauses = [f'"{ " ".join(TERM.findall(p)) }"' for p in phrases] + [f'"{t}"' for t in terms]
    if not clauses:
        raise QueryError('Search query has no searchable terms.')
    return " AND ".join(clauses)


def _page(rows: list, page: int) -> list:
    return rows[page * PAGE_SIZE:(page + 1) * PAGE_SIZE]


def new_posts(store: Store, *, expression: str | None = None,
              now: int | None = None, page: int = 0, before: int | None = None) -> list:
    """Mode 1: every eligible post (optionally lexically filtered), newest first.

    before: view the window as of that time (date jump); future values clamp to now."""
    now = timestamp(now)
    upper = min(before, now) if before else now
    match_sql, match_args = "", ()
    if expression is not None:
        match_sql = (" AND canonical_id IN (SELECT p2.canonical_id FROM posts_fts "
                     "JOIN posts p2 ON p2.rowid = posts_fts.rowid WHERE posts_fts MATCH ?)")
        match_args = (expression,)
    with store.connect() as db:
        rows = db.execute(
            f"SELECT * FROM posts WHERE created_at BETWEEN ? AND ?{match_sql} "
            "ORDER BY created_at DESC, canonical_id ASC",
            (now - WINDOW_SECONDS, upper, *match_args),
        ).fetchall()
    return _page([dict(r) for r in rows], page)


def relevance(store: Store, expression: str, *,
              now: int | None = None, page: int = 0, before: int | None = None) -> list:
    """Mode 2: lexical matches by BM25 ascending (lower is better), then recency.

    before: view the window as of that time (date jump); future values clamp to now."""
    now = timestamp(now)
    upper = min(before, now) if before else now
    with store.connect() as db:
        rows = db.execute(
            "SELECT p.*, bm25(posts_fts) AS bm25 FROM posts_fts "
            "JOIN posts p ON p.rowid = posts_fts.rowid "
            "WHERE posts_fts MATCH ? AND p.created_at BETWEEN ? AND ? "
            "ORDER BY bm25 ASC, p.created_at DESC, p.canonical_id ASC",
            (expression, now - WINDOW_SECONDS, upper),
        ).fetchall()
    return _page([dict(r) for r in rows], page)


def conversations(store: Store, *, expression: str | None = None,
                  now: int | None = None, page: int = 0, before: int | None = None) -> list:
    """Mode 3: thread cards ranked by distinct observed reply authors in 24h, then latest activity.

    Threads group by stored root_id (or the post itself when absent), so roots
    that were never collected still form provisional cards. With a query, only
    threads containing a match appear, shown on their best-BM25 post.
    """
    now = timestamp(now)
    upper = min(before, now) if before else now
    with store.connect() as db:
        rows = db.execute(
            "SELECT * FROM posts WHERE created_at BETWEEN ? AND ?",
            (now - WINDOW_SECONDS, upper),
        ).fetchall()
        match_scores: dict[str, float] = {}
        if expression is not None:
            for row in db.execute(
                "SELECT p.canonical_id AS canonical_id, bm25(posts_fts) AS bm25 FROM posts_fts "
                "JOIN posts p ON p.rowid = posts_fts.rowid "
                "WHERE posts_fts MATCH ? AND p.created_at BETWEEN ? AND ?",
                (expression, now - WINDOW_SECONDS, now),
            ):
                match_scores[row["canonical_id"]] = row["bm25"]

    threads: dict[str, list] = {}
    for row in rows:
        record = dict(row)
        threads.setdefault(record["root_id"] or record["canonical_id"], []).append(record)

    cards = []
    for root_id, members in threads.items():
        card_bm25 = None
        if expression is not None:
            matches = [m for m in members if m["canonical_id"] in match_scores]
            if not matches:
                continue
            card = min(matches, key=lambda m: (match_scores[m["canonical_id"]], m["canonical_id"]))
            card_bm25 = match_scores[card["canonical_id"]]
        else:
            card = max(members, key=lambda m: (m["created_at"], m["canonical_id"]))
        # Root author is known only when the root post itself is present.
        root_author = next((m["author_id"] for m in members if m["canonical_id"] == root_id), None)
        recent = [m for m in members
                  if m["created_at"] >= upper - DAY and m["canonical_id"] != root_id]
        reply_authors = {(m["source"], m["author_id"]) for m in recent
                         if (m["source"], m["author_id"]) != (card["source"], root_author)}
        cards.append({
            "card": card,
            "card_bm25": card_bm25,
            "root_id": root_id,
            "root_present": any(m["canonical_id"] == root_id for m in members),
            "n_reply_authors": len(reply_authors),
            "n_replies": len(members) - (1 if root_author is not None else 0),
            "latest_activity": max(m["created_at"] for m in members),
        })

    cards.sort(key=lambda c: (-c["n_reply_authors"], -c["latest_activity"], c["root_id"]))
    return _page(cards, page)


def social_discovery(store: Store, root_pubkey: str, *, expression: str | None = None,
                     now: int | None = None, page: int = 0, before: int | None = None,
                     order: str = "recent", reach: int = 2) -> list:
    """Mode 4 (Social, literal reach <= `reach` hops over the observed graph).

    Membership (bounded, observed): 1 hop = direct follows; 2 hops = authors
    in stored kind-3 lists of 1-hop authors; 3 hops = authors in stored
    kind-3 lists of observed 2-hop authors. 3-hop coverage is partial by
    construction — known graph vs absent body.

    Default order=recent: newest first within reach. order=connections ranks
    by score(c) = 1/sqrt(sum over DISTINCT first-hop endorsers e of
    1/d_path(e,c)^2), d_path = 1 (own direct edge), 2 (e lists c) or 3 (e
    lists a hub listing c, shortest path per endorser). Equal-length n paths
    reduce to the user's d/sqrt(n); direct follows keep indirect support;
    duplicate edges/cycles/self-endorsement never count twice.
    """
    import math

    now = timestamp(now)
    upper = min(before, now) if before is not None else now
    direct = store.followed(root_pubkey)
    hop_lists = {e: store.followed(e) for e in direct}
    hubs = {x for lst in hop_lists.values() for x in lst} - direct - {root_pubkey}
    hub_lists = {h: store.followed(h) for h in hubs if store.metadata(h, 3)}

    # connection mass per candidate, computed ONCE per request
    d2_endorsers: dict[str, set] = {}
    d3_endorsers: dict[str, set] = {}
    for e in direct:
        e_list = hop_lists[e]
        for c in e_list:
            if c != root_pubkey and c != e:
                d2_endorsers.setdefault(c, set()).add(e)
        for h in e_list - {root_pubkey, e}:  # no self-hub, no root cycle
            for c in hub_lists.get(h, ()):
                if c != root_pubkey and c != e:
                    d3_endorsers.setdefault(c, set()).add(e)

    def reach_of(author_id: str) -> tuple[int, float] | None:
        """(min distance, connection score); None if out of reach."""
        weight = 1.0 if author_id in direct else 0.0
        d2 = d2_endorsers.get(author_id, set())
        d3 = d3_endorsers.get(author_id, set()) - d2  # shortest path per endorser
        weight += len(d2) * (1 / 4) + len(d3) * (1 / 9)
        if weight <= 0:
            return None
        d = 1 if author_id in direct else (2 if d2 else 3)
        return (d, 1 / math.sqrt(weight))

    with store.connect() as db:
        rows = db.execute("SELECT * FROM posts WHERE created_at BETWEEN ? AND ?",
                          (now - WINDOW_SECONDS, upper)).fetchall()
        match_scores: dict[str, float] = {}
        if expression is not None:
            for row in db.execute(
                "SELECT p.canonical_id AS canonical_id, bm25(posts_fts) AS bm25 FROM posts_fts "
                "JOIN posts p ON p.rowid = posts_fts.rowid "
                "WHERE posts_fts MATCH ? AND p.created_at BETWEEN ? AND ?",
                (expression, now - WINDOW_SECONDS, now),
            ):
                match_scores[row["canonical_id"]] = row["bm25"]

    # reach map computed once per DISTINCT author, not per post row
    reach_map: dict[str, tuple[int, float]] = {}
    hits = []
    for row in rows:
        record = dict(row)
        author_id = record["author_id"]
        if author_id not in reach_map:
            reach_map[author_id] = reach_of(author_id)
        r = reach_map[author_id]
        if r is None or r[0] > reach:
            continue
        if expression is not None and record["canonical_id"] not in match_scores:
            continue
        record["hops"], record["graph_score"] = r
        record["bm25"] = match_scores.get(record["canonical_id"])
        hits.append(record)

    if order == "connections":
        # score -> recency -> id, irrespective of the query (match filter only)
        hits.sort(key=lambda r: (r["graph_score"], -r["created_at"], r["canonical_id"]))
    else:
        hits.sort(key=lambda r: (-r["created_at"], r["canonical_id"]))
    return _page(hits, page)


def source_counts(store: Store, *, now: int | None = None) -> dict[str, int]:
    """Eligible post counts per source for the status page."""
    now = timestamp(now)
    with store.connect() as db:
        rows = db.execute("SELECT source, COUNT(*) AS n FROM posts WHERE created_at BETWEEN ? AND ? "
                          "GROUP BY source", (now - WINDOW_SECONDS, now)).fetchall()
    return {row["source"]: row["n"] for row in rows}
