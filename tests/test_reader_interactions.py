"""Browser tests for the reader interactions.

Covers: search submit selects Relevance via the go submitter while an explicit
feed change applies immediately (no prev_q heuristic), tooltip beside the
selector, top-anchored expand/collapse, left/right metadata layout, and
rendering of hashtags, bare URLs, Markdown links and image placeholders.
"""

import json
import re
import time

import pytest

from meatybroth.store import Post, Store
from meatybroth.web import create_app

NOW = int(time.time())
ROOT = "6" * 64
DIRECT = "d" * 64


def make_post(source_id, author_id, text, created_at, author_name=""):
    return Post(source="nostr", source_id=source_id, author_id=author_id,
                author_name=author_name or author_id[:8], text=text,
                created_at=created_at, url="https://example.com/x",
                parent_id=None, root_id=None)


@pytest.fixture()
def store(tmp_path):
    store = Store(tmp_path / "db.sqlite")
    for i in range(25):
        store.upsert(make_post(f"crowd{i}", DIRECT, f"crowd note {i}", NOW - i), now=NOW)
    store.upsert(make_post("long1", DIRECT,
                           "intro paragraph. " + "filler sentence here. " * 60
                           + "ENDING-MARKER-near-the-end", NOW - 5), now=NOW)
    store.upsert(make_post("tags", DIRECT, "#picstr #graffiti\n## Real Heading\nhttps://blossom.example/abc", NOW - 10), now=NOW)
    store.upsert(make_post("md", DIRECT, '[— Author, "Title"](https://example.com/src) and ![](https://example.com/i.jpg)', NOW - 15), now=NOW)
    return store


@pytest.fixture()
def client(store):
    app = create_app(store, ROOT)
    app.testing = True
    return app.test_client()


def ids_in_order(html):
    return re.findall(r'<article class="post[^"]*" id="([^"]+)"', html)


# --- user action decides the mode, not query-string shape -------------------

def selected_mode(html: str) -> str | None:
    m = re.search(r'<option value="(\w+)"[^>]*selected', html)
    return m.group(1) if m else None


def test_go_submit_selects_relevance(client):
    """Go/Enter adds go=1: Relevance is selected even when the select says New."""
    body = client.get("/?q=crowd&mode=new&go=1").get_data(as_text=True)
    assert selected_mode(body) == "relevance"
    assert "match " in body  # BM25 score visible


def test_enter_submit_is_the_go_default(client):
    """Enter submits the form with its default (first) submit button: go=1."""
    body = client.get("/?q=crowd&go=1").get_data(as_text=True)
    assert selected_mode(body) == "relevance"


def test_feed_change_applies_immediately_and_keeps_q(client):
    """Selecting a feed submits without go=1: that mode applies, q preserved."""
    body = client.get("/?q=crowd&mode=new").get_data(as_text=True)
    assert selected_mode(body) == "new"
    article = re.search(r'<article class="post[^"]*".*?</article>', body, re.S).group()
    assert ids_in_order(body)[0].endswith("crowd0")  # newest first, still filtered
    assert '<span class="score">' not in article  # age alone says recency


def test_direct_query_url_still_selects_relevance(client):
    body = client.get("/?q=crowd").get_data(as_text=True)
    assert selected_mode(body) == "relevance"


def test_explicit_mode_in_direct_url_is_kept(client):
    body = client.get("/?q=crowd&mode=discovery").get_data(as_text=True)
    assert selected_mode(body) == "discovery"


def test_empty_query_relevance_falls_back_to_conversations(client):
    body = client.get("/?mode=relevance").get_data(as_text=True)
    assert selected_mode(body) == "conversations"


def test_pagination_keeps_state_without_prev_q(client):
    page = client.get("/?q=crowd&mode=new").get_data(as_text=True)
    assert "prev_q" not in page
    href = re.search(r'href="\?([^"]*)page=1"', page).group(1)
    assert "q=crowd" in href and "mode=new" in href
    assert client.get("/?" + href + "page=1").status_code == 200


# --- feed explanation tooltip ------------------------------------------------

def test_tooltip_beside_selector_not_repeated_paragraph(client):
    body = client.get("/?mode=relevance").get_data(as_text=True)
    assert 'id="feed-tip" role="tooltip"' in body
    assert 'aria-describedby="feed-tip"' in body
    assert 'class="scope-note"' not in body  # no repeated paragraph under the heading
    assert "Every post from the last 30 days, newest first." in body  # explanation still in DOM
    # the select submits without a Go click, and updates the tooltip text
    assert "this.form.submit()" in body
    assert "dataset.tip" in body


# --- expand/collapse control position ----------------------------------------

def test_expand_control_stays_above_text_in_both_states(client):
    body = client.get("/?q=filler&mode=relevance").get_data(as_text=True)
    article = re.search(r'<article class="post[^"]*" id="nostr:long1".*?</article>', body, re.S).group()
    summary = article.index('<details class="rest">')
    details_close = article.index("</details>", summary)
    preview = article.index('class="preview"')
    ending = article.index("ENDING-MARKER-near-the-end")
    # closed state: control first, then the preview; the full body sits inside
    # the expander before the preview block, hidden while closed/open by CSS.
    assert summary < ending < details_close < preview
    assert article.count("intro paragraph") == 2  # full body + hidden preview (never two visible copies)
    assert "when-open" in article and "show less" in article  # collapse stays available


def test_short_posts_have_no_expander(client):
    body = client.get("/?q=crowd1&mode=new").get_data(as_text=True)
    assert "show full post" not in body


# --- metadata line layout -----------------------------------------------------

def test_meta_identity_left_navigation_right(client):
    body = client.get("/?q=crowd&mode=relevance").get_data(as_text=True)
    article = re.search(r'<article class="post[^"]*".*?</article>', body, re.S).group()
    who, right = article.index('class="who"'), article.index('class="meta-right"')
    assert who < right
    meta = article[who:article.index('class="body"')]
    assert 'class="score"' in meta and "<time" in meta and ">replies</a>" in meta


# --- rendering of social text --------------------------------------------------

def test_hashtag_without_space_is_not_a_heading(client):
    body = client.get("/?q=picstr&mode=relevance").get_data(as_text=True)
    article = re.search(r'<article.*?</article>', body, re.S).group()
    assert "<h1>" not in article
    assert "#picstr" in article
    assert "<h2>Real Heading</h2>" in article  # genuine spaced headings still work


def test_bare_urls_and_markdown_links_and_images(client):
    body = client.get("/?q=blossom&mode=relevance").get_data(as_text=True)
    assert '<a href="https://blossom.example/abc"' in body  # bare URL clickable
    body = client.get("/?q=src&mode=relevance").get_data(as_text=True)
    assert 'href="https://example.com/src"' in body  # Markdown links keep working
    assert "[image.png]" in body  # image placeholder, not a stripped empty tag
    assert "<img" not in body  # text-only: nothing remote is fetched


def test_render_never_loses_sanitization(store, client):
    store.upsert(make_post("xss", DIRECT, '#tag <script>alert(1)</script> [x](javascript:alert(1)) <img src="https://evil/x.png">', NOW - 3), now=NOW)
    body = client.get("/?q=tag&mode=relevance").get_data(as_text=True)
    assert "<script>" not in body
    assert "<img" not in body
    assert 'javascript:' not in body
    assert "#tag" in body


# --- navigation: page jump, ±10, date jump ----------------------------------

def test_date_jump_shows_only_older_window(client, store):
    """before=<ts> limits results to the 30-day window ending there."""
    store.upsert(make_post("old1", DIRECT, "old post about activation steering",
                           NOW - 10 * 86400), now=NOW)
    body = client.get(f"/?mode=new&before={NOW - 5 * 86400}").get_data(as_text=True)
    ids = ids_in_order(body)
    assert ids and ids[0].endswith("old1")  # recent crowd posts excluded by the cutoff


def test_date_param_maps_to_utc_midnight(client, store):
    from datetime import datetime, timezone
    d = datetime.fromtimestamp(NOW - 5 * 86400, tz=timezone.utc)
    store.upsert(make_post("old2", DIRECT, "another old steering post", NOW - 6 * 86400), now=NOW)
    body = client.get(f"/?mode=new&date={d:%Y-%m-%d}").get_data(as_text=True)
    assert any(i.endswith("old2") for i in ids_in_order(body))
    # garbage date falls back to the live window
    body = client.get("/?mode=new&date=not-a-date").get_data(as_text=True)
    assert ids_in_order(body)[0].endswith("crowd0")


def test_future_before_means_live_window(client):
    body = client.get(f"/?mode=new&before={NOW + 99999}").get_data(as_text=True)
    assert ids_in_order(body)[0].endswith("crowd0")


def test_pagination_links_preserve_before_and_query(client, store):
    store.upsert(make_post("old1", DIRECT, "old post about activation steering",
                           NOW - 10 * 86400), now=NOW)
    page = client.get(f"/?q=steering&mode=relevance&before={NOW - 5 * 86400}").get_data(as_text=True)
    href = re.search(r'href="\?([^"]*)page=1"', page)
    assert href is None  # only one old match: no second page
    # with more matches than one page, links carry q/mode/before
    for i in range(25):
        store.upsert(make_post(f"oldc{i}", DIRECT, f"old crowd steering {i}",
                               NOW - 6 * 86400 - i), now=NOW)
    page = client.get(f"/?q=steering&mode=relevance&before={NOW - 5 * 86400}").get_data(as_text=True)
    href = re.search(r'href="\?([^"]*)page=1"', page).group(1)
    assert "q=steering" in href and "mode=relevance" in href and "before=" in href
    assert client.get("/?" + href + "page=1").status_code == 200
    # 'latest' drops the cutoff
    assert 'href="?q=steering&mode=relevance&page=0">latest</a>' in page  # latest drops the cutoff


def test_page_jump_and_pm10_controls(client):
    body = client.get("/?mode=new&page=1").get_data(as_text=True)
    assert 'type="number" name="page"' in body  # page-number jump input
    assert "&minus;10</a>" in body  # shown whenever page > 0, clamps at 0
    assert '<input type="hidden" name="mode" value="new">' in body
    first = client.get("/?mode=new").get_data(as_text=True)
    assert">+10</a>" in first and "older" in first  # forward links when a next page exists


# --- feed-state params survive navigation -----------------------------------

def test_reach_param_preserved_across_navigation(client):
    """Agreed passthrough set (order; reach is canonical) survives jumps;
    page/go/date do not."""
    body = client.get("/?mode=discovery&reach=2&order=connections&go=1").get_data(as_text=True)
    href = re.search(r'href="\?([^"]*)page=1"', body)
    if href:  # pagination links carry the agreed state, never go
        assert "order=connections" in href.group(1) and "go=1" not in href.group(1)
    assert '<input type="hidden" name="order" value="connections">' in body  # jump forms keep it
    # unknown params are not passed through (agreed set only)
    body = client.get("/?mode=new&junk=x").get_data(as_text=True)
    assert "junk=x" not in body


def test_mode_navigation_after_go_search(client):
    """After a Go search, a feed change (no go in the submitted form) applies."""
    body = client.get("/?q=crowd&go=1").get_data(as_text=True)
    assert selected_mode(body) == "relevance"
    # select-change submit: same params minus the go submitter
    body = client.get("/?q=crowd&mode=new").get_data(as_text=True)
    assert selected_mode(body) == "new"
    # pagination after a go search never re-sends go
    body = client.get("/?q=crowd&go=1").get_data(as_text=True)
    href = re.search(r'href="\?([^"]*)page=1"', body)
    assert href and "go" not in href.group(1)


def test_duplicate_params_canonicalized(client):
    """Repeated params collapse to one canonical value in rendered links/forms."""
    body = client.get("/?q=crowd&q=other&mode=new&mode=relevance&go=1").get_data(as_text=True)
    assert selected_mode(body) == "relevance"  # go forces relevance
    href = re.search(r'href="\?([^"]*)page=1"', body).group(1)
    assert href.count("q=") == 1 and href.count("mode=") == 1

def test_date_jump_does_not_resurrect_expired_row(client, store):
    """A stored row older than the retention window stays invisible even when a
    date jump's cutoff would otherwise cover it (window lower bound = actual now)."""
    store.upsert(make_post("expired1", DIRECT, "ancient steering post", NOW - 40 * 86400), now=NOW)
    body = client.get(f"/?mode=new&before={NOW - 10 * 86400}").get_data(as_text=True)
    assert not any(i.endswith("expired1") for i in ids_in_order(body))
    # still searchable? no: retention is fixed, the row is out of every window
    body = client.get("/?q=ancient&mode=relevance").get_data(as_text=True)
    assert not any(i.endswith("expired1") for i in ids_in_order(body))


def test_age_stays_real_under_date_jump(client, store):
    """A 10-day-old post viewed through a 5-day-ago cutoff shows its real age."""
    store.upsert(make_post("old1", DIRECT, "old post about activation steering",
                           NOW - 10 * 86400), now=NOW)
    body = client.get(f"/?mode=new&before={NOW - 5 * 86400}").get_data(as_text=True)
    article = re.search(r'<article class="post[^"]*" id="nostr:old1".*?</article>', body, re.S).group()
    assert ">10d<" in article  # real relative age, not an as-of-cutoff age


def test_future_dated_event_never_exposed(client, store):
    """Future-dated stored events stay outside every window (cutoff clamps)."""
    store.upsert(make_post("future1", DIRECT, "post dated in the future", NOW + 86400), now=NOW)
    assert not any(i.endswith("future1") for i in ids_in_order(client.get("/?mode=new").get_data(as_text=True)))
    body = client.get(f"/?mode=new&before={NOW + 99999}").get_data(as_text=True)
    assert not any(i.endswith("future1") for i in ids_in_order(body))


# --- content warnings on cards ----------------------------------------------

def _flag(store, event_id, reason, category="auto"):
    with store.connect() as db:
        db.execute("INSERT INTO content_warnings(event_id, category, reason, created_at) VALUES(?, ?, ?, ?)",
                   (event_id, category, reason, NOW))


def test_spam_flag_keeps_ordinary_preview(client, store):
    """Spam-only: small reason label, ordinary 280-char preview, no gating."""
    store.upsert(make_post("spam1", DIRECT, "intro text. " + "filler sentence here. " * 60
                           + "ENDING-SPAM", NOW - 7), now=NOW)
    _flag(store, "spam1", "auto-flagged: spam link-farm/duplicate-content")
    body = client.get("/?q=filler&mode=relevance").get_data(as_text=True)
    article = re.search(r'<article class="post[^"]*" id="nostr:spam1".*?</article>', body, re.S).group()
    assert 'class="flag"' in article and "auto-flagged: spam" in article
    assert 'class="preview"' in article  # ordinary preview
    assert 'class="warning"' not in article  # nothing gated


def test_author_cw_hides_body_behind_click_to_show(client, store):
    store.upsert(make_post("cw1", DIRECT, "flagged body about steerable activation", NOW - 7), now=NOW)
    _flag(store, "cw1", "author: nsfw art")
    body = client.get("/?q=steerable&mode=relevance").get_data(as_text=True)
    article = re.search(r'<article class="post[^"]*" id="nostr:cw1".*?</article>', body, re.S).group()
    assert 'class="warning"' in article and "author: nsfw art" in article
    assert article.index('details class="warning"') < article.index("flagged body")


def test_auto_flagged_explicit_hides_body(client, store):
    store.upsert(make_post("expl1", DIRECT, "explicit body about activation", NOW - 7), now=NOW)
    _flag(store, "expl1", "auto-flagged: explicit")
    body = client.get("/?q=activation&mode=relevance").get_data(as_text=True)
    article = re.search(r'<article class="post[^"]*" id="nostr:expl1".*?</article>', body, re.S).group()
    assert 'class="warning"' in article


def test_spam_plus_cw_still_hides(client, store):
    """Coexisting spam and CW flags: the CW gating wins."""
    store.upsert(make_post("mix1", DIRECT, "mixed flags body about activation", NOW - 7), now=NOW)
    _flag(store, "mix1", "auto-flagged: spam link-farm/duplicate-content")
    _flag(store, "mix1", "author: nsfw art", category="cw")
    body = client.get("/?q=activation&mode=relevance").get_data(as_text=True)
    article = re.search(r'<article class="post[^"]*" id="nostr:mix1".*?</article>', body, re.S).group()
    assert 'class="warning"' in article
    assert "author: nsfw art" in article and "auto-flagged: spam" in article  # both reasons labeled


def test_warning_reason_is_escaped(client, store):
    store.upsert(make_post("esc1", DIRECT, "escape test about activation", NOW - 7), now=NOW)
    _flag(store, "esc1", '<script>alert(1)</script>')
    body = client.get("/?q=activation&mode=relevance").get_data(as_text=True)
    assert "<script>alert" not in body
    assert "&lt;script&gt;" in body


def test_unflagged_post_has_no_warning_details(client):
    body = client.get("/?q=crowd&mode=relevance").get_data(as_text=True)
    article = re.search(r'<article class="post[^"]*".*?</article>', body, re.S).group()
    assert 'class="warning"' not in article and 'class="flag"' not in article


# --- social reach selector (discovery only) ----------------------------------

def test_reach_selector_only_in_discovery(client):
    body = client.get("/?mode=discovery").get_data(as_text=True)
    assert 'id="reach-select"' in body
    assert '<option value="2" selected>2 hops</option>' in body  # default matches ranking default
    body = client.get("/?mode=new").get_data(as_text=True)
    assert 'id="reach-select"' not in body


def test_reach_selection_preserved_and_canonicalized(client):
    body = client.get("/?mode=discovery&reach=3").get_data(as_text=True)
    assert '<option value="3" selected>3 hops</option>' in body
    href = re.search(r'href="\?([^"]*)page=1"', body)
    if href:
        assert "reach=3" in href.group(1)
    # invalid value falls back to the default
    body = client.get("/?mode=discovery&reach=9").get_data(as_text=True)
    assert '<option value="2" selected>2 hops</option>' in body
    # leaving discovery drops reach from links/forms
    body = client.get("/?mode=new&reach=3").get_data(as_text=True)
    assert "reach=" not in body


def test_reach_kept_on_feed_change_within_discovery(client):
    """Header form keeps the chosen reach when re-submitting from discovery."""
    body = client.get("/?mode=discovery&reach=1").get_data(as_text=True)
    assert 'id="reach-select"' in body and '<option value="1" selected>1 hop</option>' in body


# --- social reach/order selectors -------------------------------------------

ROOT2 = "6" * 64  # MEATYBROTH root pubkey in fixtures


@pytest.fixture()
def social_store(tmp_path):
    """Graph (kind-3 lists): root → [A, F]; A → [B, C]; F → [H]; H → [C].
    Posts: pA (100s), pB (200s), pC (300s), pD (400s).
    Connections scores: A 1.0 (direct), C 1/sqrt(1/4+1/9)≈1.66 (endorsed by A
    at d=2 and F at d=3 via hub H), B 2.0 (only A, d=2) — so connections order
    is pA, pC, pB; recent order is pA, pB, pC."""
    store = Store(tmp_path / "social.sqlite")
    A, B, C, D = "a" * 64, "b" * 64, "c" * 64, "d" * 64
    F, H = "f" * 64, "8" * 64
    for pubkey, followed in ((ROOT2, [A, F]), (A, [B, C]), (F, [H]), (H, [C]), (C, [D])):
        store.save_nostr_metadata(
            {"kind": 3, "pubkey": pubkey, "id": pubkey[:16] + "k3",
             "created_at": NOW - 1000,
             "tags": [["p", f] for f in followed]}, "relay")
    for sid, author, age in (("pA", A, 100), ("pB", B, 200), ("pC", C, 300), ("pD", D, 400)):
        store.upsert(make_post(sid, author, f"post by {sid} about steering", NOW - age), now=NOW)
    return store


def social_client(store):
    app = create_app(store, ROOT2)
    app.testing = True
    return app.test_client()


def short(i):
    return i[-2:]  # 'nostr:pA' -> 'pA'


def test_reach_filters_literal_hops(social_store):
    c = social_client(social_store)
    body = c.get("/?mode=discovery&reach=1").get_data(as_text=True)
    ids = ids_in_order(body)
    assert any(i.endswith("pA") for i in ids) and not any(i.endswith("pB") for i in ids)
    body = c.get("/?mode=discovery&reach=2").get_data(as_text=True)
    ids = ids_in_order(body)
    assert {short(i) for i in ids} >= {"pA", "pB", "pC"} and not any(i.endswith("pD") for i in ids)
    body = c.get("/?mode=discovery&reach=3").get_data(as_text=True)
    assert any(i.endswith("pD") for i in ids_in_order(body))


def test_order_connections_and_recent(social_store):
    c = social_client(social_store)
    body = c.get("/?mode=discovery&order=connections&reach=2").get_data(as_text=True)
    ids = ids_in_order(body)
    # C endorsed by A (d=2) and F (d=3 via hub H) beats B (one d=2 endorser)
    tags = [short(i) for i in ids]
    assert tags == ["pA", "pC", "pB"]
    body = c.get("/?mode=discovery&order=recent&reach=2").get_data(as_text=True)
    assert [short(i) for i in ids_in_order(body)] == ["pA", "pB", "pC"]  # newest first


def test_order_selector_ui_and_preservation(social_store):
    c = social_client(social_store)
    body = c.get("/?mode=discovery&order=connections&reach=3").get_data(as_text=True)
    assert 'id="order-select"' in body
    assert '<option value="connections" selected>connections</option>' in body
    href = re.search(r'href="\?([^"]*)page=1"', body)
    if href:
        assert "order=connections" in href.group(1) and "reach=3" in href.group(1)
    assert '<input type="hidden" name="order" value="connections">' in body
    # invalid order falls back to recent; leaving discovery drops both params
    body = c.get("/?mode=discovery&order=bogus").get_data(as_text=True)
    assert '<option value="recent" selected>recent</option>' in body
    body = c.get("/?mode=new&order=connections&reach=2").get_data(as_text=True)
    assert "order=" not in body and "reach=" not in body


def test_discovery_query_date_page_preserved(social_store):
    c = social_client(social_store)
    body = c.get(f"/?mode=discovery&q=steering&reach=3&order=connections&before={NOW - 86400}").get_data(as_text=True)
    href = re.search(r'href="\?([^"]*)page=1"', body)
    if href:  # only preserved when a second page exists
        for frag in ("q=steering", "reach=3", "order=connections", "before="):
            assert frag in href.group(1), frag
    # date cutoff applies within discovery too
    body = c.get(f"/?mode=discovery&reach=3&before={NOW - 350}").get_data(as_text=True)
    ids = ids_in_order(body)
    assert not any(i.endswith("pA") for i in ids)  # 100s old is newer than the cutoff
    assert any(i.endswith("pD") for i in ids)      # 400s old is inside the window


# --- discovery label/tooltip wording (single source: ranking.MODE_*) ---------

def test_discovery_label_is_just_social(client):
    body = client.get("/?mode=discovery&reach=2").get_data(as_text=True)
    assert "Social (1–2 hops)" not in body
    assert ">Social<" in body  # rendered option label from ranking.MODE_LABELS


def test_discovery_tooltip_new_wording(client):
    body = client.get("/?mode=discovery&reach=2").get_data(as_text=True)
    assert "Recent posts within the selected 1–3 hop reach." in body
    assert "incomplete follow graph" in body
    assert "1 hop), then authors they follow" not in body  # old direct-first copy gone


def test_score_detail_is_optional_title(social_store):
    body = social_client(social_store).get("/?mode=discovery&reach=2&order=connections").get_data(as_text=True)
    assert 'class="score" title="distance score: 1/distance²' in body
    # the exact formula lives only in the title attribute, not visible text
    visible = re.sub(r"<[^>]*>", "", body)
    assert "1/distance²" not in visible
