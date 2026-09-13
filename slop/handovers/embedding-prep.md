# Rust reader, embeddings, and deployment handover

— Pi/gpt-5.6-sol, 2026-09-13

## Runtime

- Permanent local URL: `http://localhost:8088/`
- Owned SQLite: `/workspace/meatybroth-rust/.local/rust-live-8086/events.sqlite`
- The current local reader is intentionally read-only on a consistent frozen database copy while the bounded Titan writer runs against `.local/rust-live-8086/events.sqlite`. Collection is paused. This is not a complete migration of the older production corpus.
- Keep port 8088 fixed. Do not configure automatic Bedrock calls in the long-running service.

## MiniLM cache

The Rust pipeline produced 2,874 post vectors (384 dimensions), 3,430 chunks, and 345,272 observed tokens. Post vectors occupy 4,414,464 bytes. Model revision is `5f1b8cd78bc4fb444dd171e59b18f3a3af89a079`; model SHA-256 is `bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5`; tokenizer SHA-256 is `da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0`.

The resumed segment embedded 1,519 posts in 6:42.69 on a Ryzen 9 5900X (max RSS 584,864 KiB). A real 255-token boundary failure was repaired by lossless recursive splitting to at most 254 content tokens. Evidence: `slop/audits/2026-09-13_minilm-backfill-window-crash.md` and `slop/verification/2026-09-13_minilm-audchk-boundary-repro.log`.

Meaning, Similar, and keyword-labelled Topics use the selected, provenance-separated vector space. MiniLM query inference is local. Titan and MiniLM vectors are never mixed.

## Titan one-off cache

The approved one-off Titan V2 settings are `amazon.titan-embed-text-v2:0`, 512 dimensions, normalized, `us-west-2`, with US$5 total and monthly ceilings. Native `aws-sdk-bedrockruntime` replaced the per-note CLI. It overlaps at most four provider awaits while all reservation/completion SQLite sections remain synchronous in one task. SDK retries are one and operation timeout is 30 seconds. Each call reserves the documented 8,192-token maximum; actual tokens settle afterward.

Authentication refreshes `cds-login` through its issuer region `us-east-2`, evaluates temporary credentials only in process memory, unsets `AWS_PROFILE`, and calls Bedrock in `us-west-2`. The bounded script is `slop/scripts/2026-09-13_resume_titan_native.sh`. It refreshes before each segment and passes an absolute expiry-minus-120-second deadline. The embedder checks that deadline before each concurrency window and every chunk; partial chunks resume in the next segment. Tests, Clippy, release build, and the two-chunk resume regression passed at immutable commit `18b4f62`.

Active job: Pueue API task `1401` (`titan-native-per-chunk-deadline`), follow process `proc_c960`. Segment 1 finished naturally at `04:19:43+08:00`: 2,188 total vectors, 11,686 eligible missing, four historical incomplete requests and US$0.016471080 succeeded cost. Segment 2 is running with a fresh 900-second credential TTL. Do not kill it mid-window; let natural deadline drains finish.

Four historical incomplete requests are excluded from automatic retry: one `uncertain` request `18209` with known 60 nUSD actual cost, and three `reserved` requests `18560`–`18562` left by a killed concurrency window, each holding the conservative 163,840 nUSD reservation. After ordinary pending work finishes, make one explicit retry for only those four event IDs. Preserve every old attempt and its actual/reserved cost. Final audit must separately report missing vectors, successful recovery requests, and historical uncertain/reserved charge range. Evidence: `slop/verification/2026-09-13_titan-killed-window-audit.log`.

Titan Similar and Topics can read cached vectors without AWS. Cached-only production disables and removes Meaning/MiniLM controls. Unsupported direct URLs return explicit HTTP 400. The original 119 rows were entirely NIP-13 proof-of-work-tagged; deterministic event-ID order now removes that partial-run ordering bias, but do not use any partial cohort for a global quality claim.

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

1. Let Pueue task `1401` run through natural credential deadlines; do not interrupt paid windows for routine patches.
2. After ordinary pending work reaches zero, recover only the four explicitly audited incomplete event IDs once, preserving prior rows and costs.
3. Audit all intended eligible IDs: vector present or explicitly historical-incomplete; report tokens, succeeded cost, held uncertain/reserved cost, and the US$5 cap.
4. Rebuild final Titan topics only after vector completion, then verify root, Social, Similar, Topics and status on the exact snapshot sequentially and concurrently.
5. Make a consistent final database snapshot for deployment. The long-running deployment remains read-only with no AWS credentials or automatic Bedrock calls.
6. TODO for a future live collector: batch-mark or hide Similar links whose selected-space vector is still pending; do not add one metadata query per card.

## Follow-up after live review

The status page now reports each embedding space's provenance, stored/eligible/pending counts, completed requests, input tokens, recorded cost, and uncertain requests. Read-only processes say collection is paused instead of claiming live collection.

Topic labels use the pinned MIT stopwords-iso English list at commit `ccc8898`. Clusters without two terms recurring in at least one fifth of posts are labelled `mixed`. The offline rebuild assigned all 119 partial Titan vectors across six topics; this does not remove the documented proof-of-work cohort bias.

Integrated SDK collection with local MiniLM started on 8088 at epoch `1789312839`; Titan stayed cached with no request path. Event `5ead12eda0399cf3200955d7dfbef8aa545ee6986f254515500172ae86456489` was created at `1789313070`, received at `1789313208`, and gained a MiniLM vector. The first Similar request overlapped a 100-post embedding batch and completed after 24.7 seconds, so the current follow-up reduces background batches to 10 and enables a 64 MiB reader page cache plus 256 MiB SQLite mmap. Retest under integrated collection before acceptance.

Live review also found whole-message `presence` and `zone_presence` envelopes in feeds. `src/posts.sql` now excludes only well-formed envelopes with the expected top-level schema from reader and embedding eligibility. Events remain in the canonical SDK store. JSON price cards and prose that quotes a presence payload remain eligible; the maintained SDK test distinguishes these cases.

The follow-up also adds a Similar heading/source-context link, preserves the Similar feed label, updates About to selected-relay collection, and puts the exact cached Titan count plus proof-of-work cohort bias in the banner. Both topic spaces were rebuilt after applying `posts.sql`. Local embedding remains in the same writer sequence as the SDK: after both relay scans and drains complete, one-post embedding transactions fill the existing 30-second interval until the next scan deadline, yielding after each chunk. There is no independent database writer. An active-drain trace found shared-model mutex wait was not the bottleneck (160 samples, mean 3.06 ms, maximum 73 ms); cold and concurrent SQLite row loading dominated. The embedding connection now uses the same 64 MiB cache and 256 MiB mmap pragmas as HTTP reads, which reduced exact-query row loading by 28–39% on an alternating same-engine benchmark without changing the query plan.

The integrated checkpoint uses one MiniLM model instead of opening a second collector copy. Its measured startup was 1.76 seconds: SDK 2 ms, posts schema 136 ms, coverage schema 41 ms, embedding schema <1 ms, social schema 758 ms, derived cleanup 171 ms, ANALYZE 254 ms, model 397 ms. Direct-follow kind-3 hydration is bounded to the configured content/profile relays; status reports stored and missing contact-list counts without claiming relay completeness.

A live review then found stale telemetry vectors written before the eligibility migration: context already returned 404, but Similar could still use the old vector as a source. The final follow-up gates Similar sources through the canonical reader query and makes `embed.sql` remove ineligible derived vectors, chunks, and topic memberships while preserving canonical events and spend history. The maintained SDK test reproduces the full save → eligible embed → injected stale vector → HTTP 400 → migration cleanup path. On the replacement integrated process, both reported telemetry IDs have eligibility 0, vector count 0, topic membership 0, context 404, and Similar 400. Topic index links use visible `Mixed #ID` discriminators without inventing labels, and the empty topic index no longer shows inert date/page controls. Status now distinguishes eligible embedded/pending counts from total stored vectors.
