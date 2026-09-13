# Next slice: integrated SDK collection

Proposed by Pi/gpt-6-astra, 2026-09-13. Design only; collection implementation is not yet assigned. Preserve the reader and all moderation/coverage requirements. No old collector transplant.

## One process, one database

`cargo run` initializes the pinned SDK SQLite store, installs the small reader schema before networking starts, then supervises the HTTP server, SDK collection and periodic maintenance together. Any indispensable task failure reaches main; no detached projector. Reader connections remain read-only. No SQL CLI, exporter, second event database or database-trait mutex wrapper.

Use `ClientBuilder::database`, `admit_policy` and `verify_subscriptions(true)` (`nostr-sdk-0.45.2/src/client/builder.rs:258–305,382–385`). The inspected SDK verifies, applies admission and saves before yielding accepted events (`relay/inner.rs:1269–1329`). Therefore collection consumes SDK streams without calling save/admission again. Recheck the published locked dependency's code at integration; the cited source is the current installed copy.

## Remove the body-copy projection

Keep canonical `events` as the only post-body store. Keep a small derived note index for stable integer FTS row IDs and validated parent/root IDs. A `posts` SQL view exposes the reader's existing shape by joining that index to canonical events and profiles. Maintain the existing Porter/BM25 FTS through event insert/delete triggers in the SDK's own transaction. Metadata and contact lists stay canonical; no copied `follows` table or periodic rebuild.

This is a proposed application schema, not an SDK extension API. SDK SQLite's connection is private (`nostr-sqlite/src/store.rs:48–50`); initialize triggers through a direct rusqlite connection before client networking. Prove FTS deletion and replacement behavior through actual SDK writes before accepting this design. Native SDK search is `INSTR(LOWER(...))`, not BM25 (`store.rs:859–877`), so removing FTS would remove a requirement.

Operational progress/list checks can live in small tables in this same file. Use bounded collection rounds; finish their SDK subscriptions before the short administrative write transactions, then start the next round. Use immediate transactions for those writes; do not add a second wrapper around every database call. Confirm contention with simultaneous reader, SDK save and maintenance tests. If this fails, fix that concrete transaction sequence rather than assuming busy_timeout solves upgrades.

## Admission and bounded maintenance

One policy checks allowed kinds, explicit operator blocks, secret patterns and current signed Primal NSFW membership before persistence. Other warning categories remain labels or click-to-show as currently specified. Signed Primal list refreshes must verify event kind/author/address/signature and distinguish a valid empty list from an unsuccessful refresh. Record signed creation time separately from check time. Bootstrap moderation before public history collection, reload operator blocks, and apply updates to already-stored events.

Use the SDK `NostrDatabase::delete(Filter)` (`nostr-database/src/lib.rs:189–192`) for retention and exclusion sweeps so canonical deletions remove derived content atomically. Keep required metadata while removing out-of-window post bodies and indexes. Do not retain rejected bodies in logs or status tables. A direct Primal custom-protocol response needs an explicit verified admission path; it is not automatically covered by SDK relay streams.

## Coverage without pretending REQ is complete

For supported relays use downward `Client::sync(filter).with(relays).opts(SyncOptions::new().direction(SyncDirection::Down))` (`client/api/sync.rs:76–105`). Bound local inventory intervals: SQLite defaults queries to 10,000 entries (`store.rs:24,602,628,900–904`). Confirm reconciliation across that boundary rather than issuing an unrestricted 30-day sync.

For non-NIP77 relays retain bounded REQ scans, EOSE/timeout distinction, and explicit unresolved same-second intervals. An admission observer records verified received timestamps/counts independently of admitted output: rejected events disappear before stream delivery (`relay/inner.rs:1272–1279`). All-rejected capped pages must neither reset history nor erase gaps. Fair retries must not let a recent unsupported gap starve older work. Unsupported coverage stays incomplete in `/status`; do not silently fall back and call it complete.

Fetch missing parents by exact event ID and profiles/contact lists for the reader's authors and reachable graph. Follow advertised relay metadata where appropriate, retain retry/error distinctions, and do not mark a failed lookup as a successful daily refresh.

## First implementation proof

Start with a local scripted relay and a fresh SQLite file. A single `cargo run` must receive a signed note, expose it through HTTP/FTS with its profile and parent, and remove it atomically after expiration or a policy update. Include forged/wrong-filter events, an NSFW author beside a benign same-topic control, a valid empty moderation list, and a capped all-rejected page above an older allowed event. Restart and confirm unresolved coverage survives. Then run the already-maintained reader regressions and paired graph matrices against this SDK-written schema.

Only after that works, migrate/refetch real retained IDs and compare content coverage as well as common IDs. Keep the original database untouched until that comparison passes. Embeddings/clusters and deployment remain later work, not hidden additions to this slice.

All dependency citations resolve under `/home/code/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/` (SDK0.45.2, SQLite0.45.1, database0.45.0). Architecture hazards are documented in `/workspace/meatybroth/slop/reviews/2026-09-13_sdk-architecture-review.md`.
