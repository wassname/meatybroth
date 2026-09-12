# Meaty Broth specification

## Purpose

Meaty Broth is a public, read-only Nostr reader for trying search and feed
algorithms on a bounded corpus. It serves text from the last 30 days and keeps
the ranking understandable.

Nostr is the only ingestion protocol. RSS, Mastodon, Bluesky, and other text can
appear only through followed bridge accounts that publish signed Nostr events.
There are no native external-site adapters, user accounts, posting, private-key
operations, images, or media downloads.

## Collection and storage

- Start from one configured public Nostr identity and its current follow list.
- Discover read relays from NIP-65 write-relay metadata, with bootstrap relays
  used only when that metadata is unavailable.
- Verify event IDs and Schnorr signatures before storing events.
- Collect short text notes, long-form text, repost targets, profiles, follow
  lists, relay metadata, and reply parents needed for visible context.
- Keep all network work inside one request budget. EOSE means a relay finished
  the request; it does not prove global completeness.
- Preserve useful replies when a parent is unavailable and show the missing
  context rather than hiding the reply.
- Expire post bodies after 30 days. Profiles, follow edges, source IDs, parent
  references, and blocklist entries can remain longer.
- Store one canonical SQLite row per Nostr event and keep the FTS index in sync.
- Report partial collection, relay failures, request-budget exhaustion, and
  filter counts on the status page.

## Feeds and search

The reader provides four distinct modes:

1. **New** — all eligible posts, newest first.
2. **Relevance** — SQLite FTS5 BM25 over the current query, with recency as the
   tie-breaker.
3. **Conversations** — thread cards ordered by distinct recent reply authors.
4. **Social** — posts from the selected 1, 2, or 3-hop follow reach. Recent
   orders by time; Connections favours authors reached through more independent
   follow paths. The page must disclose that the collected graph is incomplete.

A submitted search selects Relevance. Changing the feed, Social reach, or Social
order applies immediately. Malformed query syntax returns a useful error rather
than silently changing the query.

## Reader interaction

- Keep one compact page with search, feed controls, results, and paging.
- Each card has one metadata line, a linked author name, a source-event link,
  and a short explanation of why it ranked there.
- Feed controls have adjacent explanation tooltips.
- Long-post expansion preserves the reader's position and supports repeated
  expand/collapse without state drift.
- Render Markdown as sanitized text. Make HTTP(S) URLs clickable, but do not
  render remote images or active media.
- Provide date navigation, previous/next pages, and approximately ten-page
  jumps while preserving query and feed state.
- Context pages show an indented reply tree. A reply shown in a feed includes
  its parent when available, remains directly clickable, and links to context as
  “{n} replies”. Missing parents remain explicit.

## Content handling

- Reject obvious exposed credentials before persistence.
- Keep author content warnings and automated explicit-text flags as separate
  reasons. Hide warned text until the reader expands it.
- Keep spam visible only according to the documented operator policy.
- Apply author and event blocklists during ingestion and serving.
- Reports go through public GitHub issues. Ask only for the event ID and a short
  reason; warn reporters not to paste post content, screenshots, credentials,
  or other sensitive material.
- Removal from this reader cannot remove copies from Nostr relays.

## Deployment

One CloudFormation stack owns a public EC2 instance, encrypted root disk,
Elastic IP, DNS record, security group, and SSM role. Caddy terminates HTTPS and
proxies to the loopback Flask service. There is no SSH, NAT gateway, load
balancer, managed database, archive bucket, or backup.

The first-deployment command must deploy the exact published Git revision and
then prove all of the following:

- the stack and EC2 status checks completed;
- SSM is online;
- Docker, web, collector, and Caddy are running;
- the loopback reader returns HTTP 200;
- public HTTPS returns HTTP 200; and
- the status page reports a non-empty eligible corpus.

The current deployment is create-only. Replacing the stack deletes the only
corpus and gives the replacement a new public IP.

## Deferred

Do not add semantic/vector ranking, learned rankers, native external-site
adapters, accounts, posting, private-key custody, durable archives, backups, or
a general moderation service until actual use shows a need.
