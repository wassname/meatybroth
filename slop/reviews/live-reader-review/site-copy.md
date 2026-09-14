# Site copy draft

Requested full rewrite for the reader-facing UI. This is copy only. It does not change routes, ranking, moderation, collection, or model calls. Quoted posts, profile names/addresses, event IDs, warning reasons, and the user's tagline stay as data.

The app writer should implement the proposed strings and small structural changes below. `old → new` means replace the visible text. `delete` means remove it rather than move it elsewhere.

## Global controls: `templates/base.html`, `src/main.rs`

| location | old | new |
|:---|:---|:---|
| page title and site heading | `🍲 meatybroth.com` | preserve exactly |
| tagline | `AI slop or human broth? Who cares as long as it’s meaty` | preserve exactly |
| feed label | `Feed:` | `View:` |
| `new` label | `Latest` | `Latest` |
| `relevance` label | `Keyword search` | `Words` |
| `meaning` label | `Semantic search` | `Meaning` |
| `topics` label | `Topics` | `Topics` |
| `conversations` label | `With replies` | `With replies` |
| `discovery` label | `Network` | `Network` |
| search label | `Search:` | `Find:` |
| search placeholder | `terms AND terms, "quoted phrase"` | `words, "exact phrase"` |
| result-order label | `Results:` | `Order matches:` — show only for Words and Meaning |
| keyword button | `Keyword search` | `Words` |
| semantic button | `Semantic search` | `Meaning` |
| disabled semantic button | `Semantic search unavailable` | `Meaning unavailable` |
| disabled semantic title | `Semantic search requires a configured embedding provider` | `Meaning search is unavailable right now.` |
| vector label | `Vectors:` | `Model:` — keep visible wherever selected-space controls affect Meaning, Topics, or Similar comparisons |
| MiniLM option | `MiniLM local` | `MiniLM` |
| Titan option | `Titan` | `Titan` |
| topic clustering label | `Clusters:` | `Group by:` |
| fixed-k option | `Fixed-k` | `K-means (fixed k)` |
| network order options | `recent`, `connections` | `Newest`, `Most connected` |
| network reach label/options | `Reach:`, `1 hop`, `2 hops`, `3 hops` | `Through follows:`, `1 hop`, `2 hops`, `3 hops` |

Replace `mode_explanations` in `src/main.rs:271-277`:

```text
Latest: Recent posts, newest first.
Words: Posts containing these words. Sort by relevance or newest.
Meaning: Posts related to your query. It can find posts without the same words.
Similar posts: Posts related to this post.
Topics: Groups of related posts. Choose a topic below.
With replies: Threads with recent replies. A search limits this view to matching threads.
Network: Posts within the selected number of follow hops. Stored follow lists are incomplete.
```

Delete the scope/banner note entirely. The header already links to Status, and its operational facts belong there.

Delete the `Live SDK`, `embedded posts`, `partial backfill`, `order-biased`, `stored vectors`, `PoW`, and `AWS` wording from the main page. The controls already say which search space is selected. The status page has the diagnostics.

## Topics: `templates/feed.html`

Replace the large topic-link list with one compact **multi-select checkbox dropdown**. A single `<select>` is wrong: the user requested multiple topics. It must preserve selected topic IDs, model, clustering choice, and each count.

```html
<details class="topic-picker">
  <summary>Topics{% if selected_topics %} ({{ selected_topics|length }} selected){% endif %}</summary>
  <fieldset>
    <legend class="sr-only">Topics</legend>
    {% for topic in topics %}
    <label><input type="checkbox" name="topics" value="{{ topic.id }}"
      {% if topic.id in selected_topics %}checked{% endif %}>
      {{ topic.label }} ({{ topic.post_count }})</label>
    {% endfor %}
    <button type="submit">Apply topics</button>
  </fieldset>
</details>
```

Keep it in the existing GET controls form. Preserve repeated topic query parameters in the URL (`topics=id&topics=id`) along with `mode=topics`, `embedding`, `clustering`, page, and other active filters. The server must union selected-topic memberships, deduplicate post IDs, then apply the existing rank/order; no selected topic means the existing topic-index behavior. Do not show a long tag list below it.

Replace the topic hint with:

```text
{% if clustering == 'dbscan' %}
Groups use DBSCAN (distance {{ topic_settings.dbscan_epsilon_cosine }}, at least {{ topic_settings.dbscan_min_samples }} posts). Posts outside a group appear as Unsorted.
{% else %}
Groups use K-means with {{ topic_settings.kmeans_k }} clusters.
{% endif %}
```

Delete `Rebuilt every six hours; new vectors are assigned between rebuilds.` from the reader page. Put that timing in status details only when it is true in the deployed process.

Other feed copy:

| old | new |
|:---|:---|
| `Compared with the source post and thread context in the {{ embedding }} cache.` | `Based on <a ...>this post and its stored thread</a>.` |
| `No Social matches for this query. Search everything instead.` | `No Network matches. <a ...>Search all stored posts</a>.` |
| `Topics are not built yet.` | `No topic groups are available.` |
| `No results in the current window.` | `No posts found.` |
| `newer`, `older` | `Newer`, `Older` |
| `page`, `jump` | `Page`, `Go` |
| `jump to`, `1d ago`, `5d ago`, `go`, `latest` | `Before`, `1 day`, `5 days`, `Go`, `Latest` |
| date-jump title | `Posts before this date, limited to the last 30 days; ages are relative to today` | `Show stored posts before this date.` |

## Cards and thread pages: `templates/_post.html`, `templates/context.html`, `src/render.rs`

| location | old | new |
|:---|:---|:---|
| collapsed text | `⋯ full post (N more characters)` | `Show full post (N more characters)` |
| collapse text | `⋯ show less` | `Show less` |
| replies without count | `replies` | `Replies` |
| Similar link | `similar` | `Similar` |
| pending Similar | `similar pending` | `Similar pending` |
| pending tooltip | `This post is waiting for its {{ p.embedding }} vector` | `This post is not indexed for Similar posts.` |
| event link | `event` | `Nostr event` |
| more-button label | `more details` | `Post details` |
| details labels | `account`, `posted`, `event`, `thread`, `name` | `Account`, `Posted`, `Event`, `Thread`, `Name` |
| missing root | `root post not stored — thread context incomplete` | `The first post in this thread is not stored here.` |
| missing name | `no profile event stored yet for this account` | `No profile is stored for this account.` |
| flagged post summary | `⚠ {{ p.warning }}` | `Content warning: {{ p.warning }}` |
| visible flag | `⚠ {{ p.warning }}` | `Note: {{ p.warning }}` |
| parent marker | `↳` | preserve the marker; preserve quoted parent text |
| thread heading | `Thread context` | `Thread` |
| missing parent | `Parent post not stored (expired or never collected): ID.` | `The parent post is not stored here. ID: ID.` |
| cycle | `A stored reply cycle was omitted.` | `A reply loop was left out.` |
| current-post heading | `This post` | `Post` |
| reply heading | `N available replies` / `Available replies` | `N stored replies` / `Stored replies` |
| no replies | `No replies stored.` | `No stored replies.` |

Preserve warning reason data exactly. `author:`, `auto-flagged`, and similar strings are evidence/status labels, not site copy to rewrite in Rust. If there is a user-facing display mapping, use plain display labels there while preserving the original reason in Post details.

## Search errors and empty states: `src/main.rs`, `templates/feed.html`

Render user-caused errors as `Search: …`, not `Search error: …`. Proposed messages:

| old error | new error |
|:---|:---|
| `Search query has no searchable terms.` | `Enter a word or phrase to search.` |
| `Titan cache is not available yet` | `Titan search is not ready yet.` |
| `MiniLM cache is not configured` / `MiniLM is not configured` | `MiniLM search is not available.` |
| `Meaning search requires text` | `Enter text for Meaning search.` |
| `This Titan query is not cached and no provider is available` | `This Meaning search is not available yet.` |
| `Similar search requires an event ID` | `Choose a post first.` |
| `The source post is not eligible in the current reader window` | `This post is not available in this view.` |
| `This post is absent from the selected embedding cache` | `Similar posts are not available for this post.` |
| `Post not stored in the current window.` | `This post is not stored here.` |
| `Titan semantic search is not configured.` | `Meaning search is unavailable right now.` |
| `Titan embedding worker stopped.` | `Meaning search is temporarily unavailable.` |
| `Titan semantic search timed out.` | `Meaning search took too long. Try again later.` |
| `Reader is shutting down.` | `The reader cannot accept this request right now.` |
| `Reader request failed; see server log.` | `The reader could not load this page. Try again shortly.` |

Keep detailed causes in server logs and `/status`, not in page errors.

## About: `templates/about.html`

Replace the body with:

```html
<h2>About</h2>
<p>A reader for recent public Nostr posts and replies from selected relays. Search words, browse topics, open threads, or look for related posts.</p>
<p>We prefer human broth but take either as long as they are mildly interesting.</p>
<p>AI readers can visit <a href="https://therustyclaw.com" rel="noopener noreferrer">The Rusty Claw</a>. Source: <a href="https://github.com/wassname/meatybroth" rel="noopener noreferrer">GitHub</a>.</p>
```

## Terms: `templates/tos.html`

Retain every policy, retention, notice, disclaimer, and liability clause. This is a wording rewrite, not a substantive terms change.

```html
<h2>Terms of use</h2>
<p><strong>What this is.</strong> This experimental, non-commercial site is a public, read-only reader and search over a bounded 30-day text sample of public Nostr posts observed through the operator’s followed network and followed RSS, Mastodon, and Bluesky bridge accounts. It has no accounts or posting. Shown posts are cached copies from Nostr relays; original-event links point back to Nostr.</p>
<p><strong>What is collected and shown.</strong> Only Nostr events with a verified cryptographic signature are stored. Post text, author names, and profile details come from those events. Bodies are sanitized text with http(s) links only; they contain no scripts or images. A displayed name or address does not verify a real-world identity. Signed events cannot be edited without breaking their signatures, and this reader cannot guarantee their deletion from Nostr relays. Post bodies expire after 30 days; profiles and follow metadata can remain longer. Filter rejections retain counts and event IDs, never content.</p>
<p><strong>Prohibited material and removal.</strong> Child sexual abuse or exploitation material, exposed private credentials, threats, and doxxing are prohibited here. Obvious credentials are discarded before storage. The operator removes stored prohibited posts when found and prevents their collection. Spam and explicit content are filtered at the operator’s discretion, as described on <a href="/status">status</a>. Removing a post here does not affect relay copies or delete a signed event from Nostr network-wide.</p>
<p><strong>Notices.</strong> To report a problem, including copyright infringement, open a <a href="https://github.com/wassname/meatybroth/issues/new">GitHub issue</a> with the event ID and a short reason. Issues are public and normally show the reporter’s GitHub identity. Do not paste post text, screenshots, credentials, or illegal or sensitive material into a report. The operator reviews notices and disables public access to identified prohibited posts.</p>
<p><strong>Disclaimer and liability.</strong> This site and its contents are provided “as is”, without warranties. Shown posts are third-party Nostr content; showing one is not an endorsement, including where the operator also publishes on Nostr. To the extent permitted by law, the operator is not liable for displayed third-party posts; nothing excludes liability that cannot be excluded by law. The operator acts on notices and findings as described above. The site can pause, suspend, or stop at any time.</p>
<p><strong>Limits.</strong> Collection is budgeted and partial. Filtering can miss content or filter benign content; no particular post is guaranteed complete, correct, or available. Missing posts can result from collection limits or deliberate filtering. Author-labelled content warnings hide text until opened with the author’s reason. Unlabelled explicit text is not auto-detected and renders normally unless blocklisted. Obvious spam can be labelled rather than hidden and is never automatically deleted. The operator maintains author and post blocklists that prevent content appearing or being collected again; per-rule filter counts are on the status page.</p>
```

## Status: `templates/status.html`, `src/main.rs`

Use status for operational detail, without turning the reader into a dashboard.

```html
<h2>Status</h2>
<p>Updated {{ now }}. {{ eligible_posts }} posts are available from the last {{ window_days }} days.</p>
<div id="status-snapshot-notice"></div>
<p>Collection checks selected relays. Gaps and missing profiles are listed below.</p>

<h3>Collection</h3>
<p>{{ direct_follows - missing_contact_lists }} of {{ direct_follows }} followed accounts have a stored follow list.</p>
<!-- source reports: <details><summary>Relay reports</summary> ... JSON ... </details> -->
<!-- gaps: <details><summary>{{ gap_count }} collection gaps</summary> ... </details> -->

<h3>Content filters</h3>
<!-- moderation lists: <details><summary>List details</summary> ... </details> -->

<h3>Related-post index</h3>
{% if embedding_error %}<p>Latest indexing problem: {{ embedding_error }}</p>{% endif %}
{% for e in embedding_spaces %}
<p>{{ e.backend }}: {{ e.eligible_vectors }} posts ready, {{ e.pending }} waiting.</p>
<p>Newest ready post {{ e.newest_embedded or 'none' }}. Oldest waiting post {{ e.oldest_pending or 'none' }}.</p>
<details><summary>Model and usage details</summary>
<p>{{ e.model }}, {{ e.dimensions }} dimensions. {{ e.requests }} completed requests, {{ e.tokens }} input tokens, US${{ e.cost_usd }} recorded, US${{ e.monthly_cost_usd }} this month. {{ e.uncertain }} uncertain and {{ e.reserved }} reserved requests.</p>
<p>Revision <code>{{ e.revision }}</code><br>Model SHA-256 <code>{{ e.model_sha256 }}</code><br>Tokenizer SHA-256 <code>{{ e.tokenizer_sha256 }}</code></p>
</details>
{% else %}<p>No related-post index has been stored.</p>{% endfor %}
```

Use `Status updated …` for the cached snapshot notice. If it is old, append `It may be out of date.` Do not show `SDK`, `SQLite`, five-minute scans, cache/cohort, PoW, or AWS in the page summary.

## Link hierarchy and old Reddit reference: `static/style.css`

Reference capture, not a mockup:

- local image: [`reference-reddit-archive-2017-01-01.png`](reference-reddit-archive-2017-01-01.png), SHA-256 `22021cf2e38f54c81011fd9050173c5db13dae6b6d4b99f02dd00094ca5fded3`
- archived source: <https://web.archive.org/web/20170101000000/https://www.reddit.com/r/programming/>
- capture date reported by the archive: 2017-01-01 00:34:59 UTC (`memento-datetime`)
- inspected locally on 2026-09-14. A direct `old.reddit.com` attempt returned a network-security block and is not the reference image.

The reference makes titles and author links blue; age, score, domain, and its `comments`/`share` actions are small gray text. Its gray actions are precisely the part **not** to copy: they are difficult to identify as clickable without prior Reddit familiarity. The useful reference is the compact card hierarchy, not a complete interaction treatment. Our screenshot 36 likewise uses rust, ink, and gray for links in the same card. The author link, parent excerpt, `similar`, `replies`, `event`, and the details summary have different cues despite all being interactive.

Proposed CSS direction, keeping the broth page's paper background and its own layout:

```css
:root {
  --link: #0645ad;
  --link-hover: #0b0080;
  --metadata: #6b5f52;
}
a { color: var(--link); text-decoration: underline; }
a:visited { color: #0b0080; }
a:hover, a:focus-visible { color: var(--link-hover); }

/* metadata is gray only when it is not a link */
article.post .meta { color: var(--metadata); }
article.post .meta a.author,
article.post .meta .textlabel,
article.post .parent-excerpt { color: var(--link); text-decoration: underline; }
article.post .meta a.author { font-weight: 600; }
article.post .meta time,
article.post .meta .score,
article.post .meta .address { color: var(--metadata); }
article.post .meta details.more summary,
article.post details.rest summary { color: var(--link); text-decoration: underline; }
```

For this site, improve on the reference: use the same blue link treatment for site navigation, post links in rendered text, replies, Similar, Event, parent excerpts, pagination, and details controls. Gray then has one job: non-clickable metadata. Keep warning text amber and errors red. Do not rely on a hover-only underline. Buttons and selects stay bordered controls, separate from text links. Do not import Reddit's newsletter/sidebar clutter or its gray action-link treatment.

## Implementation inventory

Templates above cover all user-visible literals in `templates/{base,feed,_post,context,status,about,tos}.html`. Rust-generated messages to change are in `src/main.rs:245,297-341,584,808-893`. Preserve internal startup, telemetry, and server-log wording. The proposed Topic checkbox dropdown is a required structural change, not copy alone.

## Cold-reader check

A bounded external comprehension check read the controls without the current implementation context. It incorrectly inferred ActivityPub/ATProto from the previous draft, so the About copy now names Nostr. It found the search labels ambiguous; `Order matches:` is therefore limited to Words/Meaning. The user-approved selected-space control remains available for Meaning, Topics, and Similar comparisons; its plain label is `Model:`, not `Meaning with:`. `Nostr event` replaces the unexplained `Event`. Its valid remaining point is visual: gray non-link age/score must not resemble the blue author/action links. Results: [`site-copy-panel.answer.md`](../2026-09-14_glm-5.3-flash_site-copy-panel.answer.md); prompt: [`site-copy-panel-brief.md`](site-copy-panel-brief.md). The panel's claims about the existing system are not source evidence.

-- Pi/gpt-5.6-terra
