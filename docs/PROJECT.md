# Meaty Broth — a one-page explainer

## What it is

Meaty Broth is a small experiment in better Nostr search and feeds: one public,
read-only page with a feed selector and search box. It starts from one public
Nostr follow graph and keeps only a rolling month of post text.

Text from followed RSS, Mastodon, or Bluesky bridge accounts can appear, but it
arrives as signed Nostr events through the same collection path. There are no
native external-site integrations.

The broader aspiration is to recover some good parts of old Reddit across the
open social web: navigable conversations, discovery outside the direct follow
list, a chronological view that remains available, and ranking that explains
why each result appeared.

## What the reader should let me do

Open the reader, switch between New, Relevance, Conversations, and Social,
search a topic, and read posts with their replies and original-event links.
Keep it text-only, read-only, and limited to the last 30 days. No private key or
account system is needed.

One useful test is:

> Search a niche topic — “activation steering”, “representation engineering”,
> a specific alignment paper — and immediately find the five humans actually
> having the interesting conversation about it, including the replies buried
> three levels deep that ordinary search tools never surface.

Known conversations can check whether ingestion, search, and context work.
After that, use the reader and compare the algorithms. The formal benchmark in
[search-eval.md](search-eval.md) is a later tool for investigating quality, not
a prerequisite for trying a feed.

## Why it is cheap, not big

SQLite stores a bounded selection of signed Nostr events and maintains a
full-text index. A month limits age, not traffic, so collection also has an
explicit request budget and payload limits. There is no GPU or trained ranker.

Profiles and follow metadata can remain when their latest update is older than
a month. Unavailable parents stay visibly missing; useful replies do not vanish
because context is incomplete.

## The approach, honestly

- **Search and simple feeds together.** First make a small real corpus useful.
  Learned ranking can wait until there is data and a reason to use it.
- **Four distinct choices.** New is chronological, Relevance uses full-text
  search, Conversations favours threads with distinct recent repliers, and
  Social uses the collected follows-of-follows graph.
- **Social connections are not proof of humanity.** Public follows are a useful
  discovery signal, not a personhood-verification system.
- **A reply means attention, not endorsement.** Many replies do not imply
  quality, so every ranking remains inspectable.

## What is unproven

- Relay checks can expose missing or stale data but cannot establish complete
  Nostr coverage. The status page must show collection limits and gaps.
- Personal use can establish usefulness for one reader, not general quality.
- Plain text search over a well-built corpus may already be enough. The project
  should discover that before adding graph, trust, or learned-ranking machinery.

## Provenance

The repository began as a fork of The Rusty Claw, a public Nostr relay for agent
coordination. Its relay, publishing, archive, and deployment machinery is not
part of this application. Meaty Broth deploys separately with its own
CloudFormation stack.

Current requirements: [SPEC.md](../SPEC.md).
