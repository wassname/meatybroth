# Rust reader, embeddings, and deployment handover

— Pi/gpt-5.6-sol, 2026-09-13

## Runtime

- Permanent local URL: `http://localhost:8088/`
- Owned SQLite: `/workspace/meatybroth-rust/.local/rust-live-8086/events.sqlite`
- Local `http://localhost:8088/` is restored as a read-only snapshot reader. Public production is now the writable continuous Rust SDK/Titan service on the existing EC2 host.
- Continuous SDK collection, Titan embedding, and semantic search are explicitly authorized through the EC2 instance role. Keep local forwarding and the public container on port 8088.

## MiniLM cache

The Rust pipeline produced 2,874 post vectors (384 dimensions), 3,430 chunks, and 345,272 observed tokens. Post vectors occupy 4,414,464 bytes. Model revision is `5f1b8cd78bc4fb444dd171e59b18f3a3af89a079`; model SHA-256 is `bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5`; tokenizer SHA-256 is `da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0`.

The resumed segment embedded 1,519 posts in 6:42.69 on a Ryzen 9 5900X (max RSS 584,864 KiB). A real 255-token boundary failure was repaired by lossless recursive splitting to at most 254 content tokens. Evidence: `slop/audits/2026-09-13_minilm-backfill-window-crash.md` and `slop/verification/2026-09-13_minilm-audchk-boundary-repro.log`.

Meaning, Similar, and keyword-labelled Topics use the selected, provenance-separated vector space. MiniLM query inference is local. Titan and MiniLM vectors are never mixed.

## Titan backfill and continuous production

Titan V2 uses `amazon.titan-embed-text-v2:0`, 512 dimensions, normalized, in `us-west-2`. The bounded initial backfill has a US$5 total authorization. The continuous production service is separately authorized with a US$5/month safety guard; the initial US$5 total limit is not a lifetime production limit. Native `aws-sdk-bedrockruntime` replaced the per-note CLI. It overlaps at most four provider awaits while all reservation/completion SQLite sections remain synchronous in one task. SDK retries are one and operation timeout is 30 seconds. Each call reserves the documented 8,192-token maximum; actual tokens settle afterward.

Authentication refreshes `cds-login` through its issuer region `us-east-2`, evaluates temporary credentials only in process memory, unsets `AWS_PROFILE`, and calls Bedrock in `us-west-2`. The bounded script is `slop/scripts/2026-09-13_resume_titan_native.sh`. It refreshes before each segment and passes an absolute expiry-minus-120-second deadline. The embedder checks that deadline before each concurrency window and every chunk; partial chunks resume in the next segment. Tests, Clippy, release build, and the two-chunk resume regression passed at immutable commit `18b4f62`.

The local Pueue backfill stopped at a settled segment-11 boundary with 8,136 vectors. Its consistent deployment snapshot is `.local/deployment-handoff/events-8136.sqlite`, SHA-256 `cabe5372bd11729bd33468c311e9e980e5e20ddfa9bd47c973943be473cecd02`; audit: `slop/verification/2026-09-14_segment11-deployment-handoff.log`. Do not resume that local paid writer or merge a second ledger. Production continues the same canonical database and ledger.

Four historical incomplete requests are excluded from automatic retry: one `uncertain` request `18209` with known 60 nUSD actual cost, and three `reserved` requests `18560`–`18562` left by a killed concurrency window, each holding the conservative 163,840 nUSD reservation. After ordinary pending work finishes, make one explicit retry for only those four event IDs. Preserve every old attempt and its actual/reserved cost. Final audit must separately report missing vectors, successful recovery requests, and historical uncertain/reserved charge range. Evidence: `slop/verification/2026-09-13_titan-killed-window-audit.log`.

Titan Similar and Topics read stored vectors without AWS. Semantic search embeds a new query through the same serialized production worker and durable budget ledger; it never substitutes MiniLM. Unsupported direct URLs return explicit HTTP 400. The original 119 rows were entirely NIP-13 proof-of-work-tagged. Raw event-ID ascending order is deterministic and resumable but remains proof-of-work-biased while the corpus is partial. Only completion of the full intended cohort removes that selection bias; do not use any partial cohort for a global quality claim.

## Reader latency and Social

Per-card warning and reply queries were batched, semantic card lookups were bulked, and cosine search reads vector bytes directly. This reduced isolated root and MiniLM Meaning requests from 10–20 seconds to roughly 1–2 seconds.

Social initially took 15 seconds because SQLite lacked event cardinality statistics after bulk SDK ingestion. On the live DB, `ANALYZE events` took 1.79 seconds and reduced the next Social request to 3.11 seconds for 12 cards (`rank_ms=2019`, `replies_ms=193`, `cards_ms=867`). Evidence: `slop/verification/2026-09-13_social-analyze-live-probe.log`. Writable startup now runs `ANALYZE events`; the more complex proposed reach cache was discarded. Concurrent HTTP latency still needs a final measured check.

## Production deployment contract

The final service is writable and continuous:

```sh
MEATYBROTH_DB=/path/to/events.sqlite \
MEATYBROTH_ADDR=0.0.0.0:8088 \
MEATYBROTH_DEFAULT_EMBEDDING=titan \
MEATYBROTH_EMBED_BACKEND=bedrock \
MEATYBROTH_EMBED_MODEL=amazon.titan-embed-text-v2:0 \
MEATYBROTH_EMBED_DIMENSIONS=512 \
MEATYBROTH_EMBED_NORMALIZE=true \
MEATYBROTH_EMBED_MONTHLY_BUDGET_USD=5 \
AWS_REGION=us-west-2 \
/path/to/meatybroth
```

Do not set `AWS_PROFILE` or static AWS credentials on EC2. `aws-config` uses the instance role through the default credential chain and refreshes temporary role credentials. The collector and embedding ledger remain one serialized SQLite writer sequence. The deployment worker owns backup, private verification, and reversible cutover.

## Current production and remaining checks

- Public commit `cb71d87` is live. Independent root: HTTP 200 in 0.816 seconds. Actual public UAT under the writer: empty Topics 0.79 s, Similar 100 posts 7.18 s, cached Semantic 100 posts 8.75 s, and Latest 100 posts TTFB 4.99 s / total 15.09 s without requested response compression.
- `91568ab` supersedes the process-start-only scheduling candidate. It persists recent-scan admissions separately from reconciliation/backfill, prioritizes those live rows FIFO across restarts, and never lets a later history scan demote them. It removes newest-first and rollout-boundary starvation but cannot make an arrival burst faster than provider capacity.
- Full freshness acceptance is open. At 518 seconds, production admitted 1,595 eligible posts but embedded 465; 495 pending posts were already older than five minutes. Admission was about 3.08 posts/s versus 0.90 embedded posts/s. Completed-only latency was selection-biased and must not be used as the denominator.
- Preserve all provider history. Besides the original uncertain request `18209` and reserved requests `18560`–`18562`, the missing-CA deployment produced 64 uncertain attempts; a later forced stop added eight uncertain and two reserved attempts. Audit before any explicit recovery. Do not automatically retry uncertain/reserved rows.
- The runtime CA bundle is required for AWS SDK HTTPS. `cb71d87` also stops paid calls after one failed preflight, supports one-call diagnostic mode, and drains in-flight calls on SIGTERM/SIGINT before exit.
- Stop the verified duplicate legacy Python collector, preserving rollback configuration. Do not stop the Rust writer for routine checks.
- Production resource sample before that stop: CPU 52.7% average/69% maximum with no T3 credit or cgroup throttle; Rust RSS about 1.293 GiB on a 1.865 GiB host, 157 MiB available, no swap, load 3.25; database 1.029 GB plus 401.5 MB WAL; Rust block I/O 15.8 GB read/5.04 GB write. The old Python collector used another 10.4% CPU. These are observations, not a single-cause diagnosis.
- Reader connections currently allocate a 64 MiB SQLite page cache, 256 MiB mmap, and memory temp store; blocking HTTP tasks have no concurrency limit. Do not count mmap as RSS without measurement. Next inspect a two-request bound, smaller reader cache, disk-backed temp storage, and cached status aggregation. Preserve the canonical disk temp path.
- DBSCAN/fixed-k Topics and `Similar replies:` are implemented as uncommitted work in `/workspace/meatybroth-rust`; they are not deployed and remain lower priority than runtime responsiveness/freshness.
- Isolated scheduling worktree: `/workspace/.worktrees/meatybroth-sla`. Commit `91568ab` persists live/history classification and FIFO across restart. Reviewer found its partial-EOSE path marked too late; WIP fix `ff2d8ec` moves marking before the EOSE error and adds a real no-EOSE retained-arrival/reopen regression. Do not deploy `ff2d8ec` until process `proc_a93a` finishes successfully and its full test/Clippy/release logs are inspected.
- Next combined runtime work after `ff2d8ec` is green: cap blocking HTTP work at two, change read-only connections from 64 MiB cache/256 MiB mmap/memory temp toward smaller measured settings with `temp_store=FILE`, and make status accounting nonblocking/cached or index-backed without weakening dynamic moderation. Exact current logs: `slop/verification/2026-09-14_persistent-live-timeout-{test,full-tests,clippy,release}.log` inside the isolated worktree.
- This worker has no callable same-session compaction operation. The parent should compact and resume this exact session rather than launch another writer.

## Follow-up after live review

The status page now reports each embedding space's provenance, stored/eligible/pending counts, completed requests, input tokens, recorded cost, and uncertain requests. Read-only processes say collection is paused instead of claiming live collection.

Topic labels use the pinned MIT stopwords-iso English list at commit `ccc8898`. Clusters without two terms recurring in at least one fifth of posts are labelled `mixed`. The offline rebuild assigned all 119 partial Titan vectors across six topics; this does not remove the documented proof-of-work cohort bias.

Integrated SDK collection with local MiniLM started on 8088 at epoch `1789312839`; Titan stayed cached with no request path. Event `5ead12eda0399cf3200955d7dfbef8aa545ee6986f254515500172ae86456489` was created at `1789313070`, received at `1789313208`, and gained a MiniLM vector. The first Similar request overlapped a 100-post embedding batch and completed after 24.7 seconds, so the current follow-up reduces background batches to 10 and enables a 64 MiB reader page cache plus 256 MiB SQLite mmap. Retest under integrated collection before acceptance.

Live review also found whole-message `presence` and `zone_presence` envelopes in feeds. `src/posts.sql` now excludes only well-formed envelopes with the expected top-level schema from reader and embedding eligibility. Events remain in the canonical SDK store. JSON price cards and prose that quotes a presence payload remain eligible; the maintained SDK test distinguishes these cases.

The follow-up also adds a Similar heading/source-context link, preserves the Similar feed label, updates About to selected-relay collection, and puts the exact cached Titan count plus proof-of-work cohort bias in the banner. Both topic spaces were rebuilt after applying `posts.sql`. Local embedding remains in the same writer sequence as the SDK: after both relay scans and drains complete, one-post embedding transactions fill the existing 30-second interval until the next scan deadline, yielding after each chunk. There is no independent database writer. An active-drain trace found shared-model mutex wait was not the bottleneck (160 samples, mean 3.06 ms, maximum 73 ms); cold and concurrent SQLite row loading dominated. The embedding connection now uses the same 64 MiB cache and 256 MiB mmap pragmas as HTTP reads, which reduced exact-query row loading by 28–39% on an alternating same-engine benchmark without changing the query plan.

The integrated checkpoint uses one MiniLM model instead of opening a second collector copy. Its measured startup was 1.76 seconds: SDK 2 ms, posts schema 136 ms, coverage schema 41 ms, embedding schema <1 ms, social schema 758 ms, derived cleanup 171 ms, ANALYZE 254 ms, model 397 ms. Direct-follow kind-3 hydration is bounded to the configured content/profile relays; status reports stored and missing contact-list counts without claiming relay completeness.

A live review then found stale telemetry vectors written before the eligibility migration: context already returned 404, but Similar could still use the old vector as a source. The final follow-up gates Similar sources through the canonical reader query and makes `embed.sql` remove ineligible derived vectors, chunks, and topic memberships while preserving canonical events and spend history. The maintained SDK test reproduces the full save → eligible embed → injected stale vector → HTTP 400 → migration cleanup path. On the replacement integrated process, both reported telemetry IDs have eligibility 0, vector count 0, topic membership 0, context 404, and Similar 400. Topic index links use visible `Mixed #ID` discriminators without inventing labels, and the empty topic index no longer shows inert date/page controls. Status now distinguishes eligible embedded/pending counts from total stored vectors.
