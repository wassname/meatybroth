# Rust reader, embeddings, and deployment handover

— Pi/gpt-5.6-sol, 2026-09-13

## Runtime

- Permanent local URL: `http://localhost:8088/`
- Owned SQLite: `/workspace/meatybroth-rust/.local/rust-live-8086/events.sqlite`
- The current local reader is intentionally read-only while the bounded Titan run is paused. Collection coverage gaps remain visible; this is not a complete migration of the older production corpus.
- Keep port 8088 fixed. Do not configure automatic Bedrock calls in the long-running service.

## MiniLM cache

The Rust pipeline produced 2,874 post vectors (384 dimensions), 3,430 chunks, and 345,272 observed tokens. Post vectors occupy 4,414,464 bytes. Model revision is `5f1b8cd78bc4fb444dd171e59b18f3a3af89a079`; model SHA-256 is `bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5`; tokenizer SHA-256 is `da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0`.

The resumed segment embedded 1,519 posts in 6:42.69 on a Ryzen 9 5900X (max RSS 584,864 KiB). A real 255-token boundary failure was repaired by lossless recursive splitting to at most 254 content tokens. Evidence: `slop/audits/2026-09-13_minilm-backfill-window-crash.md` and `slop/verification/2026-09-13_minilm-audchk-boundary-repro.log`.

Meaning, Similar, and keyword-labelled Topics use the selected, provenance-separated vector space. MiniLM query inference is local. Titan and MiniLM vectors are never mixed.

## Titan one-off cache

The approved one-off Titan V2 settings are `amazon.titan-embed-text-v2:0`, 512 dimensions, normalized, `us-west-2`, with US$5 total and monthly ceilings. The request ledger reserves spend before each call. AWS CLI retries are disabled and connect/read timeouts are bounded.

One call and a later segment succeeded before AWS login refresh failed:

- 119 post vectors and requests
- 7,247 actual tokens
- 144,940 nano-USD = US$0.00014494
- 243,712 post-vector bytes
- no uncertain requests

The 119 rows are not representative: all are NIP-13 nonce-tagged and have IDs beginning `0000`, caused by the former raw-ID ordering. Pending work now orders newest first. Do not use this partial cohort to judge cluster quality. Preserve it for eventual full resume. Evidence: `slop/verification/2026-09-13_titan-corpus-oneoff.log`; its final joined summary is Cartesian and must not be quoted. Pre-provider auth failures are archived in `embedding_preflight_failures`, not mistaken for provider spend.

Titan Similar and Topics can read cached vectors without AWS. Titan Meaning only accepts an exact cached query; otherwise the UI returns a clear error rather than using MiniLM in the Titan space. The three intended query-cache inputs were not embedded before authentication failed.

## Reader latency and Social

Per-card warning and reply queries were batched, semantic card lookups were bulked, and cosine search reads vector bytes directly. This reduced isolated root and MiniLM Meaning requests from 10–20 seconds to roughly 1–2 seconds.

Social initially took 15 seconds because SQLite lacked event cardinality statistics after bulk SDK ingestion. On the live DB, `ANALYZE events` took 1.79 seconds and reduced the next Social request to 3.11 seconds for 12 cards (`rank_ms=2019`, `replies_ms=193`, `cards_ms=867`). Evidence: `slop/verification/2026-09-13_social-analyze-live-probe.log`. Writable startup now runs `ANALYZE events`; the more complex proposed reach cache was discarded. Concurrent HTTP latency still needs a final measured check.

## Safe deployment contract

Cached-only production must set:

```sh
MEATYBROTH_DB=/path/to/events.sqlite \
MEATYBROTH_ADDR=127.0.0.1:PORT \
MEATYBROTH_READ_ONLY=1 \
MEATYBROTH_DEFAULT_EMBEDDING=titan \
/path/to/meatybroth
```

Do not set `MEATYBROTH_EMBED_BACKEND` in the long-running cached deployment. Therefore it loads neither MiniLM nor Bedrock and needs no AWS CLI or credentials. Deploy from an immutable tested commit and copy the same SQLite file after a consistent snapshot/backup. The deployment worker owns backup, private verification, and reversible cutover.

## Remaining checks

1. Build the latest release and restart the local reader on 8088.
2. Verify root, Social, MiniLM Meaning, Similar, Topics, and partial cached Titan routes both sequentially and concurrently.
3. Run format, Clippy with warnings denied, and the full tests; capture logs.
4. Save a screenshot, ingest it, and obtain independent review.
5. Commit only owned source/evidence, report the immutable SHA to the parent and deployment worker.
6. Resume the full Titan run only after login refresh is fixed; keep the durable US$5 ledger and no automatic paid ingestion.

## Follow-up after live review

The status page now reports each embedding space's provenance, stored/eligible/pending counts, completed requests, input tokens, recorded cost, and uncertain requests. Read-only processes say collection is paused instead of claiming live collection.

Topic labels use the pinned MIT stopwords-iso English list at commit `ccc8898`. Clusters without two terms recurring in at least one fifth of posts are labelled `mixed`. The offline rebuild assigned all 119 partial Titan vectors across six topics; this does not remove the documented proof-of-work cohort bias.

Integrated SDK collection with local MiniLM started on 8088 at epoch `1789312839`; Titan stayed cached with no request path. Event `5ead12eda0399cf3200955d7dfbef8aa545ee6986f254515500172ae86456489` was created at `1789313070`, received at `1789313208`, and gained a MiniLM vector. The first Similar request overlapped a 100-post embedding batch and completed after 24.7 seconds, so the current follow-up reduces background batches to 10 and enables a 64 MiB reader page cache plus 256 MiB SQLite mmap. Retest under integrated collection before acceptance.

Live review also found whole-message `presence` and `zone_presence` envelopes in feeds. `src/posts.sql` now excludes only well-formed envelopes with the expected top-level schema from reader and embedding eligibility. Events remain in the canonical SDK store. JSON price cards and prose that quotes a presence payload remain eligible; the maintained SDK test distinguishes these cases.

The follow-up also adds a Similar heading/source-context link, preserves the Similar feed label, updates About to selected-relay collection, and puts the exact cached Titan count plus proof-of-work cohort bias in the banner. Both topic spaces were rebuilt after applying `posts.sql`. Local embedding remains in the same writer sequence as the SDK: after both relay scans and drains complete, one-post embedding transactions fill the existing 30-second interval until the next scan deadline, yielding after each chunk. There is no independent database writer. An active-drain trace found shared-model mutex wait was not the bottleneck (160 samples, mean 3.06 ms, maximum 73 ms); cold and concurrent SQLite row loading dominated. The embedding connection now uses the same 64 MiB cache and 256 MiB mmap pragmas as HTTP reads, which reduced exact-query row loading by 28–39% on an alternating same-engine benchmark without changing the query plan.

The integrated checkpoint uses one MiniLM model instead of opening a second collector copy. Its measured startup was 1.76 seconds: SDK 2 ms, posts schema 136 ms, coverage schema 41 ms, embedding schema <1 ms, social schema 758 ms, derived cleanup 171 ms, ANALYZE 254 ms, model 397 ms. Direct-follow kind-3 hydration is bounded to the configured content/profile relays; status reports stored and missing contact-list counts without claiming relay completeness.

A live review then found stale telemetry vectors written before the eligibility migration: context already returned 404, but Similar could still use the old vector as a source. The final follow-up gates Similar sources through the canonical reader query and makes `embed.sql` remove ineligible derived vectors, chunks, and topic memberships while preserving canonical events and spend history. The maintained SDK test reproduces the full save → eligible embed → injected stale vector → HTTP 400 → migration cleanup path. On the replacement integrated process, both reported telemetry IDs have eligibility 0, vector count 0, topic membership 0, context 404, and Similar 400. Topic index links use visible `Mixed #ID` discriminators without inventing labels, and the empty topic index no longer shows inert date/page controls. Status now distinguishes eligible embedded/pending counts from total stored vectors.
