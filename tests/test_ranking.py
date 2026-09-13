"""Fixture-order tests for the four feed modes."""

import pytest

from meatybroth.ranking import (PAGE_SIZE, QueryError, _load_social_graph, conversations, new_posts,
                                parse_fts, relevance, social_discovery, source_counts)
from meatybroth.store import Post, Store

NOW = 1_800_000_000
ROOT = "6" * 64
DIRECT = "d" * 64
OTHER_DIRECT = "e" * 64
TWO_HOP = "2" * 64
OUTSIDER = "f" * 64


def make_post(source, source_id, author_id, text, created_at, author_name="",
              parent_id=None, root_id=None, url="https://example.com/x"):
    return Post(source=source, source_id=source_id, author_id=author_id,
                author_name=author_name or author_id[:8], text=text,
                created_at=created_at, url=url, parent_id=parent_id, root_id=root_id)


def follow_event(pubkey, followees, created_at):
    return {"kind": 3, "pubkey": pubkey, "created_at": created_at, "id": f"{pubkey[:10]}{created_at}",
            "tags": [["p", f] for f in followees], "content": "", "sig": "fixture-only"}


@pytest.fixture()
def store(tmp_path):
    return Store(tmp_path / "db.sqlite")


def test_parse_fts_and_errors():
    assert parse_fts('activation "representation engineering" steering') == \
        '"representation engineering" AND "activation" AND "steering"'
    with pytest.raises(QueryError):
        parse_fts('unbalanced "quote')
    with pytest.raises(QueryError):
        parse_fts('""  !!!  ')


def test_parse_fts_quotes_fts5_reserved_words(store):
    """Bare uppercase AND/OR/NOT are literal search terms, not FTS5 operators."""
    for word in ("NOT", "AND", "OR"):
        assert parse_fts(word) == f'"{word}"'
    assert parse_fts('covid OR vaccine') == '"covid" AND "OR" AND "vaccine"'
    store.upsert(make_post("nostr", "n1", OTHER_DIRECT, "do NOT want", NOW - 60), now=NOW)
    rows = relevance(store, parse_fts("NOT"), now=NOW)
    assert [r["source_id"] for r in rows] == ["n1"]


def test_new_order_is_newest_first(store):
    for i, age in enumerate((500, 100, 300)):
        store.upsert(make_post("nostr", f"id{i}", "a" * 64, f"post {i}", NOW - age), now=NOW)
    rows = new_posts(store, now=NOW)
    assert [r["text"] for r in rows] == ["post 1", "post 2", "post 0"]


def test_relevance_bm25_ascending_and_requires_all_terms(store):
    store.upsert(make_post("nostr", "weak", "a" * 64, "activation steering", NOW - 100), now=NOW)
    store.upsert(make_post("nostr", "strong", "a" * 64, "activation steering activation steering", NOW - 100), now=NOW)
    store.upsert(make_post("nostr", "half", "a" * 64, "activation alone", NOW - 100), now=NOW)
    expression = parse_fts("activation steering")
    rows = relevance(store, expression, now=NOW)
    assert [r["source_id"] for r in rows] == ["strong", "weak"]  # half fails AND semantics
    with pytest.raises(QueryError):
        parse_fts('unmatched "quote')
    # quoted phrase keeps word order
    store.upsert(make_post("nostr", "rev", "a" * 64, "steering activations reversed", NOW - 100), now=NOW)
    rows = relevance(store, parse_fts('"activation steering"'), now=NOW)
    assert [r["source_id"] for r in rows] == ["strong", "weak"]  # "rev" has reversed word order


def test_conversations_distinct_authors_and_provisional_root(store):
    one, two, three = "a" * 64, "b" * 64, "c" * 64
    # Busy thread: five replies from one author.
    store.upsert(make_post("nostr", "root1", one, "thread root", NOW - 2000), now=NOW)
    for i in range(5):
        store.upsert(make_post("nostr", f"r1-{i}", two, f"same author {i}", NOW - 300 - i,
                               parent_id="nostr:root1", root_id="nostr:root1"), now=NOW)
    # Small thread: two distinct recent reply authors.
    store.upsert(make_post("nostr", "root2", one, "other root", NOW - 1000), now=NOW)
    store.upsert(make_post("nostr", "r2-a", two, "reply a", NOW - 300, parent_id="nostr:root2", root_id="nostr:root2"), now=NOW)
    store.upsert(make_post("nostr", "r2-b", three, "reply b", NOW - 200, parent_id="nostr:root2", root_id="nostr:root2"), now=NOW)
    cards = conversations(store, now=NOW)
    assert len(cards) == 2
    assert cards[0]["root_id"] == "nostr:root2"  # two distinct authors beat five from one
    assert cards[0]["n_reply_authors"] == 2
    assert cards[1]["n_reply_authors"] == 1
    # Root author's own replies do not inflate the count.
    store.upsert(make_post("nostr", "r2-c", one, "root author replies", NOW - 100,
                           parent_id="nostr:root2", root_id="nostr:root2"), now=NOW)
    assert conversations(store, now=NOW)[0]["n_reply_authors"] == 2


def test_conversations_missing_root_still_groups_and_deep_reply_matches(store):
    three = "c" * 64
    # Deep reply whose root and parent were never collected.
    store.upsert(make_post("nostr", "deep", three, "activation steering deep reply", NOW - 100,
                           parent_id="nostr:gone-parent", root_id="nostr:gone-root"), now=NOW)
    cards = conversations(store, now=NOW, expression=parse_fts("activation"))
    assert len(cards) == 1
    assert cards[0]["card"]["source_id"] == "deep"
    assert cards[0]["root_present"] is False
    # And the deep reply is itself an individual Relevance hit.
    hits = relevance(store, parse_fts("activation steering"), now=NOW)
    assert [h["source_id"] for h in hits] == ["deep"]


def test_social_recent_default_and_connections_order(store):
    """Default: newest first within reach <=3 hops. order=connections ranks by
    1/sqrt(sum 1/d_path^2): indirect support never lost, five independent 2-hop
    endorsers beat a lone direct follow."""
    store.upsert(make_post("nostr", "direct_post", DIRECT, "direct follow post", NOW - 100), now=NOW)
    store.upsert(make_post("nostr", "twohop_post", TWO_HOP, "friend of a friend", NOW - 90), now=NOW)
    # outsider: endorsed by DIRECT only (2-hop, 1 endorser)
    store.upsert(make_post("nostr", "outsider_post", OUTSIDER, "bare outsider", NOW - 80), now=NOW)
    # popular: 2-hop endorsed by SIX distinct direct follows (5 + DIRECT)
    # -> weight 6/4, score 1/sqrt(1.5) ~ 0.8165: beats lone direct (1.0)
    popular_key = "3" * 64
    store.upsert(make_post("nostr", "popular_post", popular_key, "widely endorsed", NOW - 70), now=NOW)
    endorsers = ["1" * 64, "2" * 64, "4" * 64, "5" * 64, "7" * 64]
    endorsers = [e for e in endorsers if e != TWO_HOP] + ["8" * 64]
    for i, ekey in enumerate(endorsers):
        store.save_nostr_metadata(follow_event(ekey, [popular_key], NOW - 500 - i), "wss://relay")
    store.save_nostr_metadata(follow_event(DIRECT, [TWO_HOP, OUTSIDER, popular_key], NOW - 500), "wss://relay")
    store.save_nostr_metadata(follow_event(ROOT, [DIRECT] + endorsers, NOW - 500), "wss://relay")

    # default: RECENT within reach (all four in reach, newest first)
    rows = social_discovery(store, ROOT, now=NOW)
    assert [r["source_id"] for r in rows] == ["popular_post", "outsider_post", "twohop_post", "direct_post"]  # newest first
    assert {r["hops"] for r in rows} <= {1, 2}

    # connections order: popular (0.8165) beats lone direct (1.0); twohop_post
    # and outsider_post (2.0 each, one endorser) tie -> recency breaks
    rows = social_discovery(store, ROOT, now=NOW, order="connections")
    assert [r["source_id"] for r in rows] == ["popular_post", "direct_post", "outsider_post", "twohop_post"]
    scores = [r["graph_score"] for r in rows]
    assert abs(scores[0] - 1 / 1.5 ** 0.5) < 1e-9 and abs(scores[1] - 1.0) < 1e-9
    # duplicate edges in one list don't help: duplicated p-tag test covers dedup


def test_social_duplicate_and_invalid_p_tags_count_once(store):
    """One intermediary listing an author twice (or with a malformed p tag)
    counts once, never inflating the connection score."""
    duplicated = {"kind": 3, "pubkey": DIRECT, "created_at": NOW - 500, "id": "x" * 64,
                  "tags": [["p", TWO_HOP], ["p", TWO_HOP], ["p", "zz-not-hex"], ["p", ROOT]],
                  "content": ""}
    assert store.save_nostr_metadata(duplicated, "wss://relay")
    store.save_nostr_metadata(follow_event(ROOT, [DIRECT], NOW - 600), "wss://relay")
    store.upsert(make_post("nostr", "twohop", TWO_HOP, "friend of a friend", NOW - 100), now=NOW)
    rows = social_discovery(store, ROOT, now=NOW, order="connections")
    assert [r["source_id"] for r in rows] == ["twohop"]
    assert abs(rows[0]["graph_score"] - 2.0) < 1e-9  # one distinct endorser: 1/sqrt(1/4)


def test_load_social_graph_matches_per_author_sequence(store):
    """Batched graph load returns exactly what per-author calls would."""
    hub_key, c_key = "3" * 64, "c" * 64
    store.save_nostr_metadata(follow_event(ROOT, [DIRECT, OTHER_DIRECT], NOW - 500), "wss://relay")
    store.save_nostr_metadata(follow_event(DIRECT, [TWO_HOP, hub_key], NOW - 500), "wss://relay")
    store.save_nostr_metadata(follow_event(OTHER_DIRECT, [], NOW - 500), "wss://relay")
    store.save_nostr_metadata(follow_event(TWO_HOP, [hub_key], NOW - 500), "wss://relay")
    store.save_nostr_metadata(follow_event(hub_key, [c_key], NOW - 500), "wss://relay")
    # reference: the old per-author sequence
    direct = store.followed(ROOT)
    hop_lists = {e: store.followed(e) for e in direct}
    hubs = {x for lst in hop_lists.values() for x in lst} - direct - {ROOT}
    hub_lists = {h: store.followed(h) for h in hubs if store.metadata(h, 3)}
    assert _load_social_graph(store, ROOT) == (direct, hop_lists, hub_lists)
    # empty root: no follows -> empty graph, no crash
    assert _load_social_graph(store, "9" * 64) == (set(), {}, {})


def test_social_discovery_uses_bounded_connections(store):
    """One social page must not open a connection per followed author."""
    store.save_nostr_metadata(follow_event(ROOT, [DIRECT, OTHER_DIRECT], NOW - 500), "wss://relay")
    store.save_nostr_metadata(follow_event(DIRECT, [TWO_HOP], NOW - 500), "wss://relay")
    store.upsert(make_post("nostr", "p1", DIRECT, "one hop", NOW - 100), now=NOW)
    calls = []
    raw_connect = store.connect

    @__import__("contextlib").contextmanager
    def counting():
        calls.append(1)
        with raw_connect() as db:
            yield db
    store.connect = counting
    try:
        social_discovery(store, ROOT, now=NOW, order="connections")
    finally:
        store.connect = raw_connect
    # graph load (1) + posts read (1); the old code needed 1 + follows + hubs + 1
    assert len(calls) == 2


def test_nonnostr_source_rejected_and_social_scope(store):
    """Nostr is the only ingested source now; upsert refuses everything else."""
    with pytest.raises(ValueError, match="Unsupported source"):
        store.upsert(make_post("unsupported", "post:other", "other-user", "essay", NOW - 100), now=NOW)
    store.upsert(make_post("nostr", "n1", DIRECT, "nostr activation steering note", NOW - 100), now=NOW)
    store.save_nostr_metadata(follow_event(ROOT, [DIRECT], NOW - 500), "wss://relay")
    expression = parse_fts("steering")
    assert {r["source_id"] for r in relevance(store, expression, now=NOW)} == {"n1"}
    assert {r["source_id"] for r in new_posts(store, now=NOW)} == {"n1"}
    assert [r["source_id"] for r in social_discovery(store, ROOT, now=NOW)] == ["n1"]


def test_expiry_on_reads_without_cleanup(store):
    store.upsert(make_post("nostr", "fresh", "a" * 64, "fresh", NOW - 100), now=NOW)
    assert [r["source_id"] for r in new_posts(store, now=NOW)] == ["fresh"]
    # A read at a later clock hides the post without any cleanup having run.
    later = NOW + 31 * 86400
    assert store.count(now=later) == 0
    assert new_posts(store, now=later) == []
    assert relevance(store, parse_fts("fresh"), now=later) == []
    assert conversations(store, now=later) == []
    # Age-ineligible arrivals are rejected at upsert and never stored.
    assert store.upsert(make_post("nostr", "stale", "a" * 64, "stale", NOW - 31 * 86400), now=NOW) is False
    # The body is still in the table until expire() runs; reads do the hiding.
    with store.connect() as db:
        assert db.execute("SELECT COUNT(*) FROM posts").fetchone()[0] == 1


def test_source_counts_status(store):
    store.upsert(make_post("nostr", "n1", "a" * 64, "hello", NOW - 100), now=NOW)
    assert source_counts(store, now=NOW) == {"nostr": 1}

def test_before_kwarg_date_jump_boundaries(store):
    """before clamps future values to now; rows newer than the jump are hidden;
    a row aged out by the jump never reappears."""
    store.upsert(make_post("nostr", "older", "a" * 64, "older post", NOW - 100), now=NOW)
    store.upsert(make_post("nostr", "newer", "b" * 64, "newer post", NOW - 30), now=NOW)
    store.upsert(make_post("nostr", "aging", "c" * 64, "aging post", NOW - 25 * 86400), now=NOW)
    # live window: all three visible
    assert {r["source_id"] for r in new_posts(store, now=NOW)} == {"older", "newer", "aging"}
    # jump 6 days ahead: 'aging' is 31 days old -> outside the window; never reappears
    assert set(r["source_id"] for r in new_posts(store, now=NOW + 6 * 86400)) == {"older", "newer"}  # ordering: newest first
    # past jump to NOW-50: 'newer' (30s old) is after the cutoff -> hidden
    assert [r["source_id"] for r in new_posts(store, now=NOW, before=NOW - 50)] == ["older", "aging"]
    # future before-clamps to now: same as live
    assert {r["source_id"] for r in new_posts(store, now=NOW, before=NOW + 10 * 86400)} == {"older", "newer", "aging"}
    # relevance/conversations/social honor before too: the post-jump 'newer'
    # row is excluded everywhere
    assert [r["source_id"] for r in relevance(store, parse_fts("newer"), now=NOW, before=NOW - 50)] == []
    cards = conversations(store, now=NOW, before=NOW - 50)
    assert {c["card"]["source_id"] for c in cards} == {"older", "aging"}
    # social: ROOT follows nobody in this fixture -> no candidates
    assert social_discovery(store, ROOT, now=NOW, before=NOW - 50) == []


def test_reach_param_filters_literal_distance(store):
    """reach=1/2/3 selects literal min-distance membership."""
    store.upsert(make_post("nostr", "p1", DIRECT, "one hop", NOW - 100), now=NOW)
    store.upsert(make_post("nostr", "p2", TWO_HOP, "two hops", NOW - 95), now=NOW)
    # 3-hop: DIRECT -> hub HUB (a 2-hop author with a stored list) -> C
    hub_key = "3" * 64
    c_key = "c" * 64
    store.upsert(make_post("nostr", "p3", c_key, "three hops", NOW - 90), now=NOW)
    store.save_nostr_metadata(follow_event(ROOT, [DIRECT], NOW - 500), "wss://relay")
    store.save_nostr_metadata(follow_event(DIRECT, [TWO_HOP, hub_key], NOW - 500), "wss://relay")
    store.save_nostr_metadata(follow_event(TWO_HOP, [hub_key], NOW - 500), "wss://relay")
    store.save_nostr_metadata(follow_event(hub_key, [c_key], NOW - 500), "wss://relay")

    assert [r["source_id"] for r in social_discovery(store, ROOT, now=NOW, reach=1)] == ["p1"]
    assert {r["source_id"] for r in social_discovery(store, ROOT, now=NOW, reach=2)} == {"p1", "p2"}
    assert {r["source_id"] for r in social_discovery(store, ROOT, now=NOW, reach=3)} == {"p1", "p2", "p3"}
    hops = {r["source_id"]: r["hops"] for r in social_discovery(store, ROOT, now=NOW, reach=3)}
    assert hops == {"p1": 1, "p2": 2, "p3": 3}


def test_connection_paths_shortest_per_endorser_no_cycles(store):
    """Multi-path same endorser counts the shortest; cycles/self never add;
    adding endorsers monotonically improves a candidate's score; five equal
    2-hop endorsers beat a lone direct follow (user's d/sqrt(n))."""
    # C is 2-hop via DIRECT and 3-hop via DIRECT->TWO_HOP: the same endorser
    # contributes once at the shortest path (d_path=2), no 3-hop double count.
    store.upsert(make_post("nostr", "pc", "3" * 64, "popular candidate", NOW - 80), now=NOW)
    store.save_nostr_metadata(follow_event(ROOT, [DIRECT], NOW - 500), "wss://relay")
    store.save_nostr_metadata(follow_event(DIRECT, [TWO_HOP, "3" * 64, DIRECT], NOW - 500), "wss://relay")
    store.save_nostr_metadata(follow_event(TWO_HOP, ["3" * 64, DIRECT, TWO_HOP], NOW - 500), "wss://relay")
    rows = social_discovery(store, ROOT, now=NOW, order="connections")
    assert [r["source_id"] for r in rows] == ["pc"]
    assert abs(rows[0]["graph_score"] - 2.0) < 1e-9  # one endorser, shortest path 2
    assert rows[0]["hops"] == 2

    # popular candidate: endorsers join as direct follows, one at a time
    pop = "4" * 64
    store.upsert(make_post("nostr", "pop", pop, "popular", NOW - 70), now=NOW)
    followed_so_far = [DIRECT]
    scores = []
    for i, extra in enumerate(("1" * 64, "5" * 64, "7" * 64, "8" * 64, "9" * 64)):
        store.save_nostr_metadata(follow_event(extra, [pop], NOW - 490), "wss://relay")
        # the endorser must be a ROOT direct follow for their path to count;
        # the latest-found rule replaces the whole list, so accumulate
        followed_so_far.append(extra)
        store.save_nostr_metadata(follow_event(ROOT, followed_so_far, NOW - 480 + i), "wss://relay")
        rows = social_discovery(store, ROOT, now=NOW, order="connections")
        scores.append({r["source_id"]: r["graph_score"] for r in rows}["pop"])
    # 5 distinct 2-hop endorsers: weight 5/4 -> score 2/sqrt(5) ~ 0.894 < 1.0 direct
    assert abs(scores[-1] - (2 / 5 ** 0.5)) < 1e-9
    assert all(b <= a + 1e-9 for a, b in zip(scores, scores[1:]))  # monotone improvement

