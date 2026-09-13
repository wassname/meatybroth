# Primal moderation-source distinction

Read-only audit on 2026-09-13. No network or API request was made.

## What the original and Rust readers actually use

Both use the same two signed Nostr kind-30000 parameterized-replaceable events:

- Publisher hex: `5d8282fc89410f1c57681a2c3b8be57afd1566c262fd1deb543999d39d141cb4`
- Publisher npub: `npub1tkpg9lyfgy83c4mgrgkrhzl90t732ekzvt73m6658xva88g5rj6qy6ntw4`
- `d=spam_list`: event `c788b323272348258dd49df80d229f1b7e773e7de662a68bc72a2d58e941d88e`, signed 2024-09-12, 13 unique pubkeys
- `d=nsfw_list`: event `c4bb56ddd3b476d6b0ff6266eb13ef20b34cc096b6a225feefd0679df1823843`, signed 2023-09-01, 157 `p` tags but only 143 unique pubkeys

The prior audit verified both signatures. It also found that normal relays returned no copies; the events came from Primal’s unauthenticated proprietary cache request for `parameterized_replaceable_list`. Primal’s web app names this publisher and the `spam_list` / `nsfw_list` identifiers.

Policy purpose differs by list:

- NSFW-list authors are denied from reader admission and purged from reader-visible post storage/derivatives.
- Spam-list authors remain readable with a visible Primal spam label.

The original Python database stores exactly these IDs with 143/13 set members. The Rust database stores exactly these IDs. Its status says 157 NSFW “members,” but that is the number of list entries; 14 are duplicate `p` tags. All 157 tags are valid 64-hex pubkeys. The earlier audit’s wording “143 valid / 157 raw” is therefore misleading: the actual distinction is **143 unique / 157 valid entries**, not 14 malformed entries.

## Relation to the reported 1,590 NSFW / 1,979 spam counts

I found no local event IDs, publisher, list identifiers, request transcript, or saved member set for the 1,590/1,979 figures in either repository, ignored evidence, Git history, or available session records. Therefore I cannot identify those figures as a newer version of these two signed lists.

The evidence supports these narrower conclusions:

1. There is no moderation-list loss between the original Python reader and the Rust reader: both adopted the same stale 2023/2024 Primal events.
2. The Rust status currently overstates unique NSFW authors as 157 rather than 143 because it counts duplicate tags.
3. The 1,590/1,979 figures almost certainly use a different count basis or source unless an unrecorded newer signed event exists. Plausible distinctions include counted posts rather than authors, an internal Primal classifier/export rather than the public categorized-list publisher, or another snapshot. These are hypotheses, not findings.
4. We must not claim that the dated 143/13 author lists preserve or approximate the larger moderation corpus. The earlier audit found zero matching authors and zero affected posts among 7,954 stored posts, so these lists had no observed effect on that corpus.

To compare coverage, the larger figures need one of: exact signed event IDs plus publisher and `d` tag; the Primal API request/response artifact; or the member files with a stated unit (unique author, list entry, or post). Without that provenance, count difference alone cannot establish staleness, policy equivalence, or a migration bug.

## Evidence

- `/workspace/meatybroth/.local/moderation/primal-list-audit.md`
- `/workspace/meatybroth/slop/verification/2026-09-13_sdk-collector-primal-live-probe-sqlite.txt`
- `moderation-source-distinction.txt`: direct original/Rust SQLite comparison

-- Pi/gpt-5.6-sol
