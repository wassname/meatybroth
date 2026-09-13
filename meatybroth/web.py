"""Flask reader page: one screen, four modes, URL-state feed/search/context/status.

Text only: post bodies go through Markdown -> nh3 (no img/media tags, http(s)
links only); author names, titles and all interpolated text rely on Jinja
autoescape. No JavaScript, no frontend build, no external assets.
"""

from datetime import datetime, timezone
import json
import re
from pathlib import PurePosixPath
import time
from urllib.parse import urlencode, urlparse

import nh3
from flask import Flask, abort, redirect, render_template, request
from markdown import markdown

from meatybroth import ranking
from meatybroth.ranking import MODE_EXPLANATIONS, MODE_LABELS, MODES, PAGE_SIZE
from meatybroth.store import Store, timestamp, WINDOW_SECONDS

# Media/link policy: text and safe http(s) links only; nothing remote is fetched by the page.
SANITIZER = dict(
    tags={"p", "br", "a", "code", "pre", "strong", "em", "b", "i", "ul", "ol", "li",
          "blockquote", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "table", "thead",
          "tbody", "tr", "td", "th", "span", "div", "del", "sup", "sub"},
    attributes={"a": {"href", "title"}},
    url_schemes={"http", "https"},
    link_rel="noopener noreferrer",
)

# Long posts fold into a preview + native <details> expand; full text stays in
# the store and search index (only the RENDERING is split, never the stored body).
PREVIEW_CHARS = 280
HEXISH_NAME = re.compile(r"[0-9a-f]{8,64}")

# ATX headings need a space after the hashes (CommonMark), but python-markdown
# does not enforce that, so social hashtags like "#picstr" become headings.
# Escape those hashes (outside fenced code) before rendering.
HEADING_HASHES = re.compile(r"^(\s*(?:>\s*)*)(#{1,6})(?![ \t#])", re.M)
FENCE = re.compile(r"^\s*(?:```|~~~)")
IMAGE_MD = re.compile(r"!\[([^\]]*)\]\(([^)\s]+)\)")
# Bare http(s) URLs in rendered text nodes -> clickable links (angle autolinks).
BARE_URL = re.compile(r"https?://[^\s<>\"']+", re.I)
TRAILING_PUNCT = ".,;:!?\u201d\u2019'\""


def _outside_fences(text: str) -> list[tuple[int, int]]:
    """Line ranges (start, end) outside ``` / ~~~ fenced code blocks."""
    spans, start, in_fence = [], 0, False
    for line in text.splitlines(keepends=True):
        end = start + len(line)
        if FENCE.match(line):
            if not in_fence:
                spans.append((start, end))
            in_fence = not in_fence
            start = end
            continue
        if not in_fence:
            spans.append((start, end))
        start = end
    return spans


def _prep_markdown(text: str) -> str:
    """Fence-aware fixes for social-text habits python-markdown mishandles:
    leading '#hashtag' (no space) must not be a heading, and '![](url)' images
    (stripped by the sanitizer anyway) must not silently vanish."""
    out = list(text)
    for a, b in _outside_fences(text):
        segment = "".join(out[a:b])
        segment = HEADING_HASHES.sub(lambda m: f"{m.group(1)}\\{m.group(2)}", segment)
        segment = IMAGE_MD.sub(lambda m: f"[\\[{_image_label(m.group(1))}\\]]", segment)
        out[a:b] = list(segment)
    return "".join(out)


def _image_label(alt: str) -> str:
    alt = alt.strip().replace("[", "(").replace("]", ")")
    return f"image: {alt}" if alt else "image.png"


def _linkify_text_nodes(html: str) -> str:
    """Wrap bare http(s) URLs in rendered text nodes with <a> (before nh3 re-clean).
    Skips existing <a> content and <code>/<pre> blocks; the sanitizer runs after."""
    parts = re.split(r"(<[^>]+>)", html)
    a_depth = code_depth = 0
    out = []
    for part in parts:
        if part.startswith("<"):
            low = part.lower()
            if low.startswith("<a"):
                a_depth += 1
            elif low.startswith("</a"):
                a_depth = max(0, a_depth - 1)
            elif low.startswith(("<code", "<pre")):
                code_depth += 1
            elif low.startswith(("</code", "</pre")):
                code_depth = max(0, code_depth - 1)
            out.append(part)
        elif a_depth or code_depth:
            out.append(part)
        else:
            out.append(BARE_URL.sub(_url_anchor, part))
    return "".join(out)


def _url_anchor(match: re.Match) -> str:
    raw = match.group()
    url = raw
    while url and url[-1] in TRAILING_PUNCT:
        url = url[:-1]
    # Trailing ')' belongs to the prose unless the URL itself contains '('.
    if url.endswith(")") and url.count("(") < url.count(")"):
        url = url[:-1]
    label, after = raw[:len(url)], raw[len(url):]
    return f'<a href="{url}">{label}</a>{after}'

BECH32 = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"


def npub(pubkey_hex: str) -> str:
    """Account npub (Bech32) for profile links; raises ValueError on bad input."""
    data = bytes.fromhex(pubkey_hex)
    if len(data) != 32:
        raise ValueError("pubkey must be 32 bytes")
    acc = bits = 0
    chars = []
    for byte in data:
        acc = (acc << 8) | byte
        bits += 8
        while bits >= 5:
            bits -= 5
            chars.append(BECH32[(acc >> bits) & 31])
    if bits:
        chars.append(BECH32[(acc << (5 - bits)) & 31])
    # Bech32 checksum = polymod(hrp_expansion + data + [0]*6) XOR 1.
    check = 1
    expanded = [ord(c) >> 5 for c in "npub"] + [0] + [ord(c) & 31 for c in "npub"] \
        + [BECH32.index(c) for c in chars] + [0, 0, 0, 0, 0, 0]
    for v in expanded:
        top = check >> 25
        check = ((check & 0x1FFFFFF) << 5) ^ v
        for i, g in enumerate((0x3B6A57B2, 0x26508E6D, 0x1EA119FA, 0x3D4233DD, 0x2A1462B3)):
            if (top >> i) & 1:
                check ^= g
    check ^= 1
    return "npub1" + "".join(chars) + "".join(BECH32[(check >> (5 * (5 - i))) & 31] for i in range(6))


def render_body(text: str) -> str:
    """Untrusted post text -> sanitized HTML (no scripts, no img, no remote media)."""
    return nh3.clean(_linkify_text_nodes(markdown(_prep_markdown(text), extensions=["fenced_code"])),
                     **SANITIZER)


def safe_link(url: str) -> str | None:
    """Original-source link only when it is an absolute http(s) URL."""
    if not url:
        return None
    scheme = urlparse(url).scheme
    return url if scheme in ("http", "https") else None


def fmt_time(created_at: int) -> str:
    return datetime.fromtimestamp(created_at, tz=timezone.utc).strftime("%Y-%m-%d %H:%M UTC")


def fmt_age(created_at: int, now: int) -> str:
    age = max(0, now - created_at)
    for unit, size in (("d", 86400), ("h", 3600), ("m", 60)):
        if age >= size:
            return f"{age // size}{unit}"
    return f"{age}s"


def _warning_hides(reasons: list[str]) -> bool:
    """Author CWs and auto-flagged explicit gate the body behind click-to-show;
    spam flags only get a small label (the card keeps its ordinary preview)."""
    return any(r.startswith(("author:", "auto-flagged: explicit", "curated-nsfw:")) for r in reasons)


def _post_view(row: dict, now: int, score: str, store: Store, score_title: str = "") -> dict:
    """One stored row -> compact card view: profile link/address, preview split,
    one inline score. Profile is resolved at rendering time, so a kind-0 event
    that arrived after the post updates the name without refetching content."""
    author = row["author_name"].strip()
    # Ingest stores a hex prefix as a placeholder name; treat hex-like names as
    # unnamed and resolve the real profile at rendering time.
    if not author or HEXISH_NAME.fullmatch(author):
        author = ""
    address = ""
    event = store.metadata(row["author_id"], 0)
    if event:
        try:
            profile = json.loads(event.get("content", ""))
            author = str(profile.get("display_name") or profile.get("name") or author)
            nip05 = str(profile.get("nip05") or "").strip()
            if "@" in nip05:
                # A NIP-05 like _@domain is the root identifier of that domain.
                address = ("@" + nip05.split("@", 1)[1]) if nip05.startswith("_") else nip05
        except (ValueError, AttributeError):
            pass
    profile_url = f"https://njump.me/{npub(row['author_id'])}"
    text = row["text"]
    # Render the COMPLETE body once for the expanded state; the preview is a
    # separate safe render of the leading text, hidden while expanded (no
    # Markdown construct is ever split across two renders' link/code spans).
    full_html = render_body(text)
    preview_html = render_body(text[:PREVIEW_CHARS]) if len(text) > PREVIEW_CHARS else ""
    return {
        "source": row["source"],
        "source_id": row["source_id"],
        "canonical_id": row["canonical_id"],
        "parent_id": row["parent_id"],
        "warning": "",
        "warning_hides": False,
        "reply_count": None,
        "parent_excerpt": None,
        "author": author or row["author_id"][:16],
        "named": bool(author),
        "address": address,
        "profile_url": profile_url,
        "author_id": row["author_id"],
        "when": fmt_time(row["created_at"]),
        "age": fmt_age(row["created_at"], now),
        "original_url": safe_link(row["url"]),
        "full_html": full_html,
        "preview_html": preview_html,
        "rest_chars": max(0, len(text) - PREVIEW_CHARS),
        "score": score,
        "score_title": score_title,
    }


def _attach_card_extras(store: Store, views: list[dict], now: int, *, parent_excerpts: bool) -> None:
    """Attach stored-only counts, CW state and optional parent excerpts to cards."""
    if not views:
        return
    ids = [v["canonical_id"] for v in views]
    event_ids = [v["source_id"] for v in views if v["source"] == "nostr"]
    reasons = store.content_warnings(event_ids)
    for view in views:
        rs = reasons.get(view["source_id"], []) if view["source"] == "nostr" else []
        view["warning"] = " · ".join(rs)
        view["warning_hides"] = _warning_hides(rs)

    marks = ",".join("?" * len(ids))
    with store.connect() as db:
        counts = dict(db.execute(
            f"SELECT parent_id, COUNT(*) FROM posts WHERE parent_id IN ({marks}) "
            "AND created_at BETWEEN ? AND ? GROUP BY parent_id",
            (*ids, now - WINDOW_SECONDS, now),
        ))
        for view in views:
            # None means unknown/unavailable, not an asserted global zero.
            view["reply_count"] = counts.get(view["canonical_id"])

        if not parent_excerpts:
            return
        parent_ids = sorted({v["parent_id"] for v in views if v["parent_id"]})
        if not parent_ids:
            return
        pmarks = ",".join("?" * len(parent_ids))
        parents = [dict(row) for row in db.execute(
            f"SELECT * FROM posts WHERE canonical_id IN ({pmarks}) "
            "AND created_at BETWEEN ? AND ?",
            (*parent_ids, now - WINDOW_SECONDS, now),
        )]
    parent_views = [_post_view(row, now, "", store) for row in parents]
    _attach_card_extras(store, parent_views, now, parent_excerpts=False)
    by_id = {v["canonical_id"]: v for v in parent_views}
    for view in views:
        parent = by_id.get(view["parent_id"])
        if parent is None:
            continue
        # full_html is sanitized; turn its paragraph/tag boundaries into spaces.
        text = "content warning" if parent["warning_hides"] else re.sub(
            r"\s+", " ", re.sub(r"<[^>]+>", " ", parent["full_html"])
        ).strip()
        if len(text) > 160:
            text = text[:159].rstrip() + "…"
        view["parent_excerpt"] = {
            "source": parent["source"], "source_id": parent["source_id"],
            "author": parent["author"], "text": text,
        }


def _context_tree(store: Store, post: dict, now: int) -> tuple[list[dict], list[tuple[dict, int]], str | None, bool]:
    """Stored parent ancestry plus descendants; root_id is not used as ancestry."""
    ancestors, seen = [], {post["canonical_id"]}
    missing_parent_id, cycle_cut = None, False
    parent_id = post["parent_id"]
    while parent_id:
        if parent_id in seen:
            cycle_cut = True
            break
        parent = store.get(parent_id, now=now)
        if parent is None:
            missing_parent_id = parent_id
            break
        ancestors.append(parent)
        seen.add(parent_id)
        parent_id = parent["parent_id"]
    ancestors.reverse()

    start = now - WINDOW_SECONDS
    with store.connect() as db:
        rows = [dict(row) for row in db.execute(
            """
            WITH RECURSIVE descendants(canonical_id, path) AS (
                SELECT canonical_id, char(31) || ? || char(31) || canonical_id || char(31)
                FROM posts WHERE parent_id = ? AND created_at BETWEEN ? AND ?
              UNION ALL
                SELECT child.canonical_id, descendants.path || child.canonical_id || char(31)
                FROM posts AS child JOIN descendants ON child.parent_id = descendants.canonical_id
                WHERE child.created_at BETWEEN ? AND ?
                  AND instr(descendants.path, char(31) || child.canonical_id || char(31)) = 0
            )
            SELECT posts.* FROM descendants JOIN posts USING (canonical_id)
            """,
            (post["canonical_id"], post["canonical_id"], start, now, start, now),
        )]

    children: dict[str, list[dict]] = {}
    for row in rows:
        children.setdefault(row["parent_id"], []).append(row)
    for siblings in children.values():
        siblings.sort(key=lambda row: (row["created_at"], row["canonical_id"]))
    descendants: list[tuple[dict, int]] = []
    visited = set(seen)
    stack = [(child, 1) for child in reversed(children.get(post["canonical_id"], []))]
    while stack:
        child, depth = stack.pop()
        cid = child["canonical_id"]
        if cid in visited:
            cycle_cut = True
            continue
        visited.add(cid)
        descendants.append((child, min(depth, 6)))
        stack.extend((grandchild, depth + 1) for grandchild in reversed(children.get(cid, [])))
    return ancestors, descendants, missing_parent_id, cycle_cut


def _int_arg(name: str, default: int) -> int:
    raw = request.args.get(name)
    if raw is None:
        return default
    try:
        value = int(raw)
    except ValueError:
        return default
    return max(0, value)


def _before_arg(now: int) -> int | None:
    """Date-jump request param -> cutoff unix seconds, else None (live window).

    `before` is unix seconds; `date` is YYYY-MM-DD (UTC midnight). Same
    invalid-input convention as _int_arg: unusable values mean the live window.
    """
    before = _int_arg("before", 0)
    if not before:
        raw_date = request.args.get("date", "")
        try:
            before = int(datetime.strptime(raw_date, "%Y-%m-%d")
                         .replace(tzinfo=timezone.utc).timestamp())
        except ValueError:
            return None
    # A future cutoff is the live window, not an empty page.
    return before if 0 < before < now else None


def create_app(store: Store, root_pubkey: str) -> Flask:
    app = Flask(__name__)
    app.config["root_pubkey"] = root_pubkey

    def _context_common():
        return {
            "modes": MODES,
            "mode_labels": MODE_LABELS,
            "mode_explanations": MODE_EXPLANATIONS,
            "window_days": WINDOW_SECONDS // 86400,
        }

    @app.get("/")
    @app.get("/search")
    def feed():
        # /search is an alias so a bookmarkable topic is always the / URL state.
        if request.path == "/search":
            return redirect("/?" + request.query_string.decode(), code=301)
        q = request.args.get("q", "").strip()
        mode = request.args.get("mode", "")
        page = _int_arg("page", 0)
        # Date jump: the cutoff bounds results (upper = min(before, now) inside
        # ranking) while the retention bound and displayed ages stay anchored at
        # the ACTUAL now, so expired rows cannot reappear and ages stay real.
        now = timestamp()
        before = _before_arg(now)
        # Social graph distance (discovery only): literal 1/2/3-hop filter;
        # dropped from links/forms outside discovery so it can't leak state.
        reach = _int_arg("reach", 2)
        if reach not in (1, 2, 3):
            reach = 2
        order = request.args.get("order", "recent")
        if order not in ("recent", "connections"):
            order = "recent"
        if mode != "discovery":
            reach = order = None
        # User actions, not query-string shape, decide the mode: submitting the
        # search (Go button or Enter -> the form's submitter adds go=1) selects
        # Relevance; changing the Feed select submits WITHOUT go=1 and applies
        # that algorithm immediately while keeping q. Direct bookmarked URLs
        # with a q but no explicit mode keep behaving as Relevance searches.
        if q and "go" in request.args:
            mode = "relevance"
        elif mode not in MODES:
            mode = "relevance" if q else "conversations"
        if not q and mode == "relevance":
            mode = "conversations"

        error = None
        expression = None
        results = []
        if q:
            try:
                expression = ranking.parse_fts(q)
            except ranking.QueryError as exc:
                error = str(exc)
        if error is None:
            if mode == "new":
                rows = ranking.new_posts(store, expression=expression, now=now, page=page, before=before)
                # Age already says recency: no score on New cards.
                results = [_post_view(r, now, "", store) for r in rows]
            elif mode == "relevance":
                rows = ranking.relevance(store, expression, now=now, page=page, before=before)
                results = [_post_view(r, now, f"match {r['bm25']:.1f} (lower=closer)", store)
                           for r in rows]
            elif mode == "conversations":
                cards = ranking.conversations(store, expression=expression, now=now, page=page, before=before)
                for card in cards:
                    score = f"{card['n_reply_authors']} repliers (24h)"
                    if expression:
                        score += f" · match {card['card_bm25']:.1f}"
                    view = _post_view(card["card"], now, score, store)
                    view["conversation"] = {
                        "n_reply_authors": card["n_reply_authors"],
                        "n_replies": card["n_replies"],
                        "latest": fmt_age(card["latest_activity"], now),
                        "root_present": card["root_present"],
                        "root_id": card["root_id"],
                    }
                    results.append(view)
            elif mode == "discovery":
                rows = ranking.social_discovery(store, root_pubkey, expression=expression,
                                                now=now, page=page, before=before,
                                                reach=reach, order=order)
                for r in rows:
                    score_title = ("distance score: 1/distance² over each distinct "
                                   "first-hop account that endorses this author; "
                                   "lower ranks first")
                    if r["hops"] == 1:
                        score = "1 hop · you follow"
                    else:
                        score = f"{r['hops']} hops · connection {r['graph_score']:.2f}"
                    if expression:
                        score += f" · match {r['bm25']:.1f}"
                    results.append(_post_view(r, now, score, store, score_title=score_title))
        else:
            results = []

        _attach_card_extras(store, results, now, parent_excerpts=True)

        has_next = len(results) == PAGE_SIZE
        # Preserve only recognized feed-state params (agreed set) across
        # All agreed feed-state params (q/mode/before/reach/order) are
        # canonicalized above, so nothing needs raw passthrough — the
        # mechanism stays for future params. page/go/date are never carried
        # (go would wrongly re-trigger the search action, date duplicates
        # before).
        passthrough = {}
        qs_items = {**passthrough, "q": q, "mode": mode, "before": before, "reach": reach, "order": order}
        qs = urlencode({k: v for k, v in qs_items.items() if v})
        qs_base = qs + "&" if qs else ""
        return render_template(
            "feed.html",
            q=q, mode=mode, page=page, error=error, results=results,
            has_next=has_next, qs_base=qs_base,
            now=now, before=before, reach=reach, order=order,
            state_fields=sorted(passthrough.items()),
            **_context_common(),
        ), (400 if error else 200)

    @app.get("/context/<source>/<path:source_id>")
    def context(source: str, source_id: str):
        canonical_id = f"{source}:{source_id}"
        now = timestamp()
        post = store.get(canonical_id, now=now)
        if post is None:
            abort(404)
        ancestors, descendants, missing_parent_id, cycle_cut = _context_tree(store, post, now)
        ancestor_views = [_post_view(row, now, "", store) for row in ancestors]
        view = _post_view(post, now, "", store)
        reply_views = []
        for row, depth in descendants:
            reply = _post_view(row, now, "", store)
            reply["tree_depth"] = depth
            reply_views.append(reply)
        _attach_card_extras(store, ancestor_views + [view] + reply_views, now, parent_excerpts=False)
        return render_template(
            "context.html", post=view, ancestors=ancestor_views, replies=reply_views,
            available_reply_count=len(descendants),
            missing_parent_id=missing_parent_id, cycle_cut=cycle_cut,
            **_context_common(),
        )

    @app.get("/about")
    def about():
        return render_template("about.html", **_context_common())

    @app.get("/tos")
    def tos():
        return render_template("tos.html", **_context_common())

    @app.get("/status")
    def status():
        now = timestamp()
        counts = ranking.source_counts(store, now=now)
        rows = []
        for row in store.status():
            rows.append({**row,
                         "eligible_posts": counts.get(row["source"], 0),
                         "updated": fmt_time(row["updated_at"])})
        return render_template("status.html", status_rows=rows, now=fmt_time(now),
                               **_context_common())

    return app
