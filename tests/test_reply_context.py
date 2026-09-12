"""Stored-parent reply context and feed excerpts."""

import re
import time

import pytest

from meatybroth.store import Post, Store
from meatybroth.web import create_app

NOW = int(time.time())
ROOT = "6" * 64
AUTHOR = "a" * 64


def post(source_id, text, age, parent_id=None, root_id=None):
    return Post("nostr", source_id, AUTHOR, "Author", text, NOW - age,
                "https://example.com/" + source_id, parent_id, root_id)


@pytest.fixture()
def store(tmp_path):
    db = Store(tmp_path / "tree.sqlite")
    db.upsert(post("root", "root token", 1000), now=NOW)
    db.upsert(post("parent", "PARENT-BODY-SAFE parent token", 900, "nostr:root", "nostr:root"), now=NOW)
    db.upsert(post("target", "targettoken current post", 800, "nostr:parent", "nostr:root"), now=NOW)
    db.upsert(post("reply1", "childtoken first reply", 700, "nostr:target", "nostr:root"), now=NOW)
    db.upsert(post("reply2", "childtoken second reply", 600, "nostr:target", "nostr:root"), now=NOW)
    db.upsert(post("nested", "nested child", 500, "nostr:reply1", "nostr:root"), now=NOW)
    previous = "nostr:target"
    for depth in range(1, 9):
        source_id = f"deep{depth}"
        db.upsert(post(source_id, f"deep child {depth}", 400 - depth, previous, "nostr:root"), now=NOW)
        previous = f"nostr:{source_id}"
    db.upsert(post("orphan", "orphan reply", 300, "nostr:not-collected", "nostr:gone-root"), now=NOW)
    # Stored ancestry cycle for a separate target; no recursive walk may hang.
    db.upsert(post("cycle1", "cycle one", 200, "nostr:cycle2"), now=NOW)
    db.upsert(post("cycle2", "cycle two", 199, "nostr:cycle1"), now=NOW)
    db.upsert(post("cycletarget", "cycle target", 198, "nostr:cycle1"), now=NOW)
    return db


@pytest.fixture()
def client(store):
    app = create_app(store, ROOT)
    app.testing = True
    return app.test_client()


def article(html, source_id):
    return re.search(rf'<article class="post[^>]* id="nostr:{source_id}".*?</article>', html, re.S).group()


def test_feed_parent_excerpt_and_actual_stored_reply_count(client):
    feed = client.get("/?q=targettoken&mode=relevance").get_data(as_text=True)
    target = article(feed, "target")
    assert ">3 replies</a>" in target  # three direct stored children, no global claim
    assert 'class="parent-excerpt" href="/context/nostr/parent"' in target
    assert "PARENT-BODY-SAFE parent token" in target
    leaf_feed = client.get("/?q=childtoken&mode=relevance").get_data(as_text=True)
    leaf = article(leaf_feed, "reply2")
    assert ">replies</a>" in leaf and "0 replies" not in leaf
    one_child = article(leaf_feed, "reply1")
    assert ">1 reply</a>" in one_child


def test_parent_excerpt_respects_cw_gate(client, store):
    with store.connect() as db:
        db.execute("INSERT INTO content_warnings(event_id, category, reason, created_at) VALUES (?, ?, ?, ?)",
                   ("parent", "author-cw", "author: nsfw art", NOW))
    feed = client.get("/?q=targettoken&mode=relevance").get_data(as_text=True)
    target = article(feed, "target")
    assert "content warning" in target
    assert "PARENT-BODY-SAFE" not in target  # author content warning hides parent text


def test_context_uses_stored_parent_tree_with_capped_indent(client):
    page = client.get("/context/nostr/target").get_data(as_text=True)
    # Actual ancestry is root -> parent -> target; root_id does not build the tree.
    assert page.index('id="nostr:root"') < page.index('id="nostr:parent"') < page.index('id="nostr:target"')
    assert "11 available replies" in page  # direct + all stored descendants
    assert 'id="nostr:reply1"' in page and 'style="--tree-depth: 1"' in page
    assert 'id="nostr:nested"' in page and 'style="--tree-depth: 2"' in page
    assert 'id="nostr:deep8"' in page and 'style="--tree-depth: 6"' in page  # visual indent cap


def test_context_handles_chain_deeper_than_python_recursion_limit(client, store):
    previous = "nostr:target"
    for depth in range(1105):
        source_id = f"long{depth}"
        store.upsert(post(source_id, "long stored reply", 1000 + depth, previous, "nostr:root"), now=NOW)
        previous = f"nostr:{source_id}"
    page = client.get("/context/nostr/target").get_data(as_text=True)
    assert 'id="nostr:long1104"' in page
    assert page.count('style="--tree-depth: 6"') > 1000


def test_conversations_score_is_the_context_link_with_no_separate_replies_link(client):
    page = client.get("/?mode=conversations").get_data(as_text=True)
    cards = [m.group(0) for m in re.finditer(r'<article class="post[\s\S]*?</article>', page)]
    scored = [c for c in cards if "repliers (24h)" in c]
    assert scored  # at least one thread card carries the repliers score
    for card in scored:
        assert re.search(r'<a class="score textlabel" href="/context/[^\"]+"[^>]*>[^<]*repliers \(24h\)</a>', card)
        assert ">replies</a>" not in card and ">1 reply</a>" not in card  # no redundant link
    # Other modes keep the direct-reply link.
    relevance = client.get("/?q=targettoken&mode=relevance").get_data(as_text=True)
    assert ">3 replies</a>" in article(relevance, "target")


def test_context_labels_missing_parent_and_cuts_cycles(client):
    orphan = client.get("/context/nostr/orphan").get_data(as_text=True)
    assert "Parent post not stored" in orphan and "nostr:not-collected" in orphan
    cycle = client.get("/context/nostr/cycletarget").get_data(as_text=True)
    assert cycle.count('id="nostr:cycle1"') == 1
    assert "A stored reply cycle was omitted." in cycle


def test_context_cards_preserve_warning_gate(client, store):
    with store.connect() as db:
        db.execute("INSERT INTO content_warnings(event_id, category, reason, created_at) VALUES (?, ?, ?, ?)",
                   ("reply1", "explicit", "auto-flagged: explicit", NOW))
    page = client.get("/context/nostr/target").get_data(as_text=True)
    reply = article(page, "reply1")
    assert 'class="warning"' in reply and "auto-flagged: explicit" in reply
