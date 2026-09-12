"""HTTP-level tests for the reader page, including sanitization."""

import json
import re
import threading
import time

import pytest

from meatybroth.store import Post, Store
from meatybroth.web import create_app

NOW = int(time.time())
ROOT = "6" * 64
DIRECT = "d" * 64
OTHER_DIRECT = "e" * 64
TWO_HOP = "2" * 64
NAMED = "b" * 64


def make_post(source, source_id, author_id, text, created_at, author_name="",
              parent_id=None, root_id=None, url="https://example.com/x"):
    return Post(source=source, source_id=source_id, author_id=author_id,
                author_name=author_name or author_id[:8], text=text,
                created_at=created_at, url=url, parent_id=parent_id, root_id=root_id)


def follow_event(pubkey, followees, created_at):
    return {"kind": 3, "pubkey": pubkey, "created_at": created_at, "id": f"{pubkey[:10]}{created_at}",
            "tags": [["p", f] for f in followees], "content": "", "sig": "fixture-only"}


def profile_event(pubkey, name, nip05=None, created_at=None):
    content = {"display_name": name}
    if nip05:
        content["nip05"] = nip05
    return {"kind": 0, "pubkey": pubkey, "created_at": created_at or NOW - 400,
            "id": f"{pubkey[:10]}profile", "tags": [], "content": json.dumps(content),
            "sig": "fixture-only"}


@pytest.fixture()
def store(tmp_path):
    store = Store(tmp_path / "db.sqlite")
    # Direct-follow posts deliberately crowd out a full page.
    for i in range(25):
        store.upsert(make_post("nostr", f"crowd{i}", DIRECT, f"crowd note {i}", NOW - i), now=NOW)
    store.upsert(make_post("nostr", "twohop", TWO_HOP, "friend of a friend mentions activation steering",
                           NOW - 100, url="https://example.com/twohop"), now=NOW)
    # A named account (stored kind-0 profile with NIP-05) posting recently.
    store.upsert(make_post("nostr", "named1", NAMED, "named account essay on activation steering",
                           NOW - 50, url="https://example.com/named"), now=NOW)
    store.save_nostr_metadata(profile_event(NAMED, "Named Author", "named@example.com"), "wss://relay")
    # Deep reply whose root and parent were never collected.
    store.upsert(make_post("nostr", "deep", TWO_HOP, "deep reply about representation engineering",
                           NOW - 80, parent_id="nostr:gone-parent", root_id="nostr:gone-root"), now=NOW)
    store.save_nostr_metadata(follow_event(DIRECT, [TWO_HOP], NOW - 500), "wss://relay")
    store.save_nostr_metadata(follow_event(ROOT, [DIRECT, NAMED], NOW - 500), "wss://relay")
    return store


@pytest.fixture()
def client(store):
    app = create_app(store, ROOT)
    app.testing = True
    return app.test_client()


def ids_in_order(html):
    return re.findall(r'<article class="post[^"]*" id="([^"]+)"', html)


def test_default_is_conversations_and_query_selects_relevance(client):
    body = client.get("/").get_data(as_text=True)
    assert "Conversations" in body  # default view: live threads first (user request)
    assert "repliers (24h)" in body
    # New stays reachable explicitly, newest first.
    new_body = client.get("/?mode=new").get_data(as_text=True)
    assert ids_in_order(new_body)[0].endswith("crowd0")
    body = client.get("/?q=activation").get_data(as_text=True)
    assert "Relevance" in body
    assert re.search(r'<option value="relevance"[^>]*selected', body)
    first = ids_in_order(body)[0]
    assert first in ("nostr:twohop", "nostr:deep", "nostr:named1")


def test_four_modes_genuinely_differ(client):
    relevance_ids = ids_in_order(client.get("/?q=activation&mode=relevance").get_data(as_text=True))
    social_ids = ids_in_order(client.get("/?q=activation&mode=discovery").get_data(as_text=True))
    conversations_html = client.get("/?mode=conversations").get_data(as_text=True)
    new_ids = ids_in_order(client.get("/?mode=new").get_data(as_text=True))
    assert social_ids and all(cid.startswith("nostr:") for cid in social_ids)
    # Conversations renders compact cards with the replier score inline.
    assert "repliers (24h)" in conversations_html
    assert new_ids[0].endswith("crowd0") and relevance_ids != new_ids[:len(relevance_ids)]


def test_social_recent_default_with_score_labels(client):
    """Default Social order is recent within reach; per-row labels show the
    connection basis (user superseded hard direct-first ordering)."""
    body = client.get("/?q=activation&mode=discovery").get_data(as_text=True)
    # named1 (NOW-50) newer than twohop (NOW-100) -> recent order
    assert ids_in_order(body) == ["nostr:named1", "nostr:twohop"]
    assert "1 hop · you follow" in body
    assert "connection" in body


def test_social_without_query_includes_crowd_first(client):
    body = client.get("/?mode=discovery").get_data(as_text=True)
    ids = ids_in_order(body)
    assert ids[0].endswith("crowd0")  # direct follows included, newest first
    assert "1 hop · you follow" in body
    all_pages = [i for pg in range(3) for i in
                 ids_in_order(client.get(f"/?mode=discovery&page={pg}").get_data(as_text=True))]
    assert "nostr:twohop" in all_pages


def test_profile_link_address_and_fallback(client):
    """Named account: display name links to the npub profile URL, NIP-05 address shown."""
    body = client.get("/?q=named&mode=relevance").get_data(as_text=True)
    assert 'href="https://njump.me/npub1' in body
    assert ">Named Author</a>" in body
    assert "@example.com" in body
    # Unnamed account: hex fallback plus explicit no-profile note in the menu.
    body = client.get("/?q=friend&mode=relevance").get_data(as_text=True)
    assert "no profile event stored yet" in body


def test_delayed_profile_updates_name_without_refetch(tmp_path):
    """Post stored before its profile: the name appears once kind-0 arrives."""
    store = Store(tmp_path / "db.sqlite")
    store.upsert(make_post("nostr", "p1", NAMED, "content before profile", NOW - 60), now=NOW)
    app = create_app(store, ROOT)
    app.testing = True
    client = app.test_client()
    body = client.get("/?q=content&mode=relevance").get_data(as_text=True)
    assert "bbbbbbbb" in body  # hex fallback, not a name
    store.save_nostr_metadata(profile_event(NAMED, "Late Named", "late@example.com",
                                           created_at=NOW - 10), "wss://relay")
    body = client.get("/?q=content&mode=relevance").get_data(as_text=True)
    assert "Late Named" in body  # rendered name updated, same stored post
    assert "no profile event stored yet" not in body


def test_mode_switch_keeps_query(client):
    q = "q=activation+steering"
    social = client.get(f"/?{q}&mode=discovery").get_data(as_text=True)
    assert "nostr:" in "".join(ids_in_order(social))
    relevance = client.get(f"/?{q}&mode=relevance").get_data(as_text=True)
    assert "nostr:named1" in ids_in_order(relevance)
    assert "Named Author" in relevance


def test_deep_reply_with_absent_root(client):
    body = client.get("/?q=representation&mode=relevance").get_data(as_text=True)
    assert "nostr:deep" in ids_in_order(body)  # missing parents do not hide the reply
    page = client.get("/context/nostr/deep").get_data(as_text=True)
    assert "Parent post not stored" in page
    assert "nostr:gone-parent" in page
    assert "deep reply about representation engineering" in page


def test_malformed_query_shows_visible_error(client):
    response = client.get('/?q=' + '%22' + 'unclosed')
    assert response.status_code == 400
    body = response.get_data(as_text=True)
    assert "Search error" in body
    assert "No results in the current window" not in body
    # Junk with no searchable terms also errors instead of an empty success.
    response = client.get("/?q=%22%22%20%21%21%21")
    assert response.status_code == 400


def test_malformed_page_arg_clamps(client):
    assert client.get("/?page=abc").status_code == 200
    assert client.get("/?page=-3").status_code == 200


def test_xss_no_script_no_img_no_remote_media(client, store):
    store.upsert(make_post("nostr", "evil1", "9" * 64,
                           '<script>alert(1)</script>hello ![pic](https://evil.example/x.png) '
                           '<img src="https://evil.example/x.png">'
                           ' [link](javascript:alert(2))',
                           NOW - 10, author_name='<b>Evil</b> <script>name()</script>'), now=NOW)
    store.upsert(make_post("nostr", "evil2", "9" * 64, "profile bait",
                           NOW - 10, author_name='x" onmouseover="alert(3)'), now=NOW)
    body = client.get("/?mode=new").get_data(as_text=True)
    assert "<script" not in body
    assert "<img" not in body
    assert "alert(1)" not in body
    assert "javascript:" not in body
    assert "https://evil.example" not in body  # no remote media URL survives into the page
    assert "&lt;b&gt;Evil&lt;/b&gt;" in body  # profile is escaped, not rendered


def test_about_page(client):
    body = client.get("/about").get_data(as_text=True)
    assert "we prefer human broth but take either as long as they are mildly interesting" in body
    assert 'href="https://therustyclaw.com"' in body
    assert "AIs: visit our sister site" in body
    # The feed page has no footer text.
    assert "missing context is labelled" in re.sub(r"\s+", " ", body)
    feed = client.get("/").get_data(as_text=True)
    assert "Missing context is labelled" not in feed
    # Visible About navigation from the feed page; site name links the feed.
    assert 'href="/about"' in feed
    assert '<a href="/">🍲 meatybroth.com</a>' in feed
    assert "AI slop or human broth? Who cares as long as it’s meaty" in feed  # italic tagline row
    assert "Feed:" in feed  # feed selector label with native tooltip
    assert 'title="Posts matching your terms' in feed or all(
        f'title="{e}"' in feed for e in (
            "Every post from the last 30 days, newest first.",
            "Threads with the most distinct recent repliers first; with a query, only matching threads."))
    assert "<footer>" not in feed


def test_tos_page(client):
    body = client.get("/tos").get_data(as_text=True)
    flat = re.sub(r"\s+", " ", body)
    assert "read-only" in flat
    assert "no posting" in flat
    assert "proof of who wrote it" in flat
    # Collector, rendering, filtering, and collection-limit claims.
    assert "public, read-only reader and search" in flat
    assert "cached copies" in flat
    assert "signature verifies" in flat
    assert "derived from those signed events" in flat
    assert "no scripts or images in post bodies" in flat
    assert "expire and disappear after 30 days" in flat
    assert "profile and follow metadata may be kept beyond that window" in flat
    assert "without breaking its signature" in flat
    assert "discarded before being stored" in flat
    assert "click-to-show" in flat
    assert "flagged text is hidden behind a click-to-show" in flat
    assert "blocklist of authors and posts" in flat
    assert "prevents it from being collected again" in flat
    assert "filter counts are visible on the status page" in flat
    assert "Missing posts may reflect collection limits or deliberate filtering" in flat
    # Prohibited-material and removal policy.
    assert "Child sexual abuse and exploitation material, exposed private credentials, and threats or doxxing" in flat
    assert "filtered at the operator's discretion" in flat
    assert "disables public access" in flat
    # Reports use GitHub issues and do not ask for copied sensitive material.
    assert '<a href="https://github.com/wassname/meatybroth/issues/new">github.com/wassname/meatybroth/issues/new</a>' in flat
    assert "the post's event ID and a short reason" in flat
    assert "GitHub issues are public" in flat
    assert "Do not paste post content, screenshots or credentials" in flat
    assert "do not copy illegal or sensitive material anywhere" in flat
    assert "Removal from this site does not" in flat
    # No invented contact/agent, consent claims, or unscoped render claims.
    assert "re-host" not in flat
    assert "remove or edit content at its source" not in flat
    assert "linked from every shown name" not in flat
    assert "verbatim" not in flat
    assert "no JavaScript" not in flat
    assert "cannot be globally deleted" not in flat
    assert "collection gaps, not curation" not in flat
    assert "bridging choice" not in flat
    assert "opted in" not in flat
    assert "respond expeditiously" not in flat
    assert "DMCA" not in flat
    assert "safe harbor" not in flat
    assert "immunity" not in flat
    assert "designated copyright agent" not in flat
    assert "will be listed here once designated" not in flat  # pending-contact placeholder removed
    assert "@" not in flat  # no invented email/contact addresses
    assert 'href="/tos"' in client.get("/").get_data(as_text=True)


def test_mode_explanation_in_tooltip(client):
    for mode, fragment in (("new", "newest first"),
                           ("relevance", "lower = closer"),
                           ("conversations", "distinct recent repliers"),
                           ("discovery", "1–3 hop reach")):
        q = "&q=activation" if mode in ("relevance", "discovery") else ""
        body = client.get(f"/?mode={mode}{q}").get_data(as_text=True)
        assert 'id="feed-tip" role="tooltip"' in body, mode
        assert fragment in body, mode


def test_long_post_folds_into_expandable_preview(client, store):
    ending = "ENDING-MARKER-near-the-end"
    long_text = "intro paragraph. " + "filler sentence here. " * 60 + ending
    store.upsert(make_post("nostr", "long1", DIRECT, long_text, NOW - 5), now=NOW)
    body = client.get("/?q=filler&mode=relevance").get_data(as_text=True)
    assert ending not in body.split("details")[0]  # preview does not contain the far ending
    assert ending in body  # full text still rendered (inside the expander)
    assert "show full post" in body and "more characters" in body
    # Short posts get no expander.
    short_body = client.get("/?q=crowd1&mode=new&prev_q=crowd1").get_data(as_text=True)
    assert "show full post" not in short_body


def test_status_page_lists_sources(client, store):
    store.set_status("nostr", {"follows": 1}, now=NOW)
    body = client.get("/status").get_data(as_text=True)
    assert "nostr" in body


def test_search_alias_redirects_preserving_state(client):
    response = client.get("/search?q=activation&mode=relevance")
    assert response.status_code == 301
    assert response.headers["Location"].startswith("/?q=activation")


def test_pagination(client):
    page0 = client.get("/?mode=new").get_data(as_text=True)
    page1 = client.get("/?mode=new&page=1").get_data(as_text=True)
    assert ids_in_order(page0) != ids_in_order(page1)
    assert "older" in page0
    first_page1 = ids_in_order(page1)[0]
    assert first_page1 == "nostr:crowd20"  # page 1 continues the recency order without overlap


def test_older_link_traversal_preserves_query_state(client):
    """Following the rendered 'older' href keeps q/mode/prev_q."""
    import html as html_mod
    for url in ("/?q=crowd&mode=new&prev_q=crowd",  # queried New, 25 matches
                "/?q=crowd&mode=relevance"):  # relevance filter
        page = client.get(url)
        assert page.status_code == 200
        html = html_mod.unescape(page.get_data(as_text=True))
        href = re.search(r'href="\?([^"]*)page=1"', html)
        assert href, f"no older link on {url}"
        assert ("q=crowd" in href.group(1)) == ("q=crowd" in url)
        assert ("mode=new" in href.group(1)) == ("mode=new" in url)
        followed = client.get("/?" + href.group(1) + "page=1").get_data(as_text=True)
        ids = ids_in_order(followed)
        assert ids, f"page 2 empty when following older from {url}"


def test_browser_feed_no_external_requests(store):
    from urllib.parse import urlparse
    from werkzeug.serving import make_server
    app = create_app(store, ROOT)
    server = make_server("127.0.0.1", 0, app)
    port = server.server_port
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        from playwright.sync_api import sync_playwright
        with sync_playwright() as p:
            browser = p.chromium.launch()
            page = browser.new_page()
            hosts = set()
            page.on("request", lambda r: hosts.add(urlparse(r.url).hostname))
            page.goto(f"http://127.0.0.1:{port}/?mode=new", wait_until="networkidle")
            browser.close()
        assert hosts == {"127.0.0.1"}
    finally:
        server.shutdown()
