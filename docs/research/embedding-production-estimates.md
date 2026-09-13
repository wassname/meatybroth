# Embedding production estimates

Date: 2026-09-13; updated 2026-09-14 with frozen partial-run and host observations. This updates the original Python-era estimate for the Rust SQLite schema. Prices are USD.

## Answer

At the observed base rate, one retained month is **918,000 text notes**. Titan V2 API input remains provisionally **$1.49–$2.21/month** using two MiniLM token proxies. Continuous production Titan embedding is authorized with separate $5 setup/backfill and $5 monthly limits. The original 119-call run measured 7,247 Titan tokens and $0.00014494, but `ORDER BY event_id` selected proof-of-work notes. A later frozen 8,136-success-event snapshot has a larger 7,537-event matched eligible cohort: 155.11 Titan tokens/post and an *illustrative* $2.85/base-month sensitivity. It remains a low-ID/PoW-biased partial cohort, and excludes 599 successful eligible Titan events without a MiniLM match; neither measurement calibrates the retained-corpus forecast.

The current schema stores each chunk vector and one aggregate vector per post. SQLite allocation is therefore materially larger than `notes × dimensions × 4`:

| space | retained DB↓ | +25% working room↓ | API/month↓ | use |
|---|---:|---:|---:|---|
| *MiniLM 384-fp32* | *5.72 GiB* | *7.15 GiB* | *$0* | measured local schema |
| Titan V2 512-fp32 | 8.80 GiB | 11.00 GiB | ~$1.49–$2.21 | projected; partial run incomplete |

These totals include measured Rust note/event/index/FTS density, measured MiniLM-table allocation and synthetic Titan-table allocation. They exclude profiles, follows, collection history, backups, model cache and a future ANN index. The current 30 GiB volume is probably enough for one month. Bedrock runs inference outside EC2, so embeddings alone do not justify a larger instance. Production web/collection CPU and RAM are still unmeasured, so this does not establish that the whole service needs no upgrade.

## Inputs and arithmetic

The volume source is the NostrMash snapshot preserved in the original repository: 30,600 text notes/day from 12 active tracked relays. It is one indexer's coverage, not all Nostr.

- Retained notes: $30{,}600\;\text{notes/day} \times 30\;\text{days} = 918{,}000\;\text{notes}$.
- Sensitivity: 459,000 at ×0.5; 2,754,000 at ×3.
- Length sample: 7,937 non-empty posts, mean 81.2 MiniLM tokens/post and 2,334,145 UTF-8 bytes total.
- MiniLM chunking: the provider limit is 256 tokens. The completed Rust database has 3,430 chunks / 2,874 embedded posts = 1.193 chunks/post. The earlier corpus artifact measured 1.169.
- Titan chunking: the current 8,000-byte Rust splitter produces 7,982 chunks / 7,937 non-empty sampled posts = 1.0057 chunks/post; 17 posts need more than one chunk.

Raw float32 vector payload is:

$$S_{raw} = N_{posts} \cdot (1 + c_{chunks/post}) \cdot d_{dimensions} \cdot 4\;\text{bytes}$$

where the `1` is the aggregate post vector retained in addition to chunk vectors. This gives 2.88 GiB for measured MiniLM chunking and 3.51 GiB for projected Titan chunking. The completed MiniLM database allocates 14,680,064 bytes across its embedding tables and indexes for 2,874 posts; scaling the visible ratio gives 4.37 GiB. A 10,000-post synthetic fill of the Titan schema gives 7.44 GiB. Titan's 512-float rows cross a SQLite overflow-page boundary with the current 4 KiB page size, which explains why allocated storage grows by more than the 512/384 dimension ratio.

The non-embedding Rust note sample measured 1,586.9 bytes/note after subtracting the empty schema. Scaling this event, tag-index, reader-row and FTS footprint adds 1.36 GiB. The 25% column allows for WAL, fragmentation and ordinary growth; it is not a substitute for the excluded metadata measurements.

The request ledger does not expire with posts. Measured MiniLM allocation grows by about 0.28 GiB/month; synthetic Titan allocation grows by about 0.22 GiB/month at base volume. For Titan that is about 2.6 GiB/year even though vectors retain only 30 days. This ledger must survive host replacement because it is also the evidence used by the cumulative spending limit.

## What is observed today

Observed 2026-09-13; [historical SQLite inputs](../../slop/verification/2026-09-13_embedding-storage-observations.log) and [completed MiniLM evidence](../../slop/verification/2026-09-13_rust-minilm-measured-estimate-evidence.log).

| database | file state | rows | interpretation |
|---|---:|---:|---|
| Original reader | 27.18 MiB | 7,937 posts | Selected 30-day corpus; post/FTS density was previously measured at 1.36 KiB/post. |
| Rust active reader | 49.48 MiB main + 12.39 MiB WAL | 3,126 text notes at first completed read; 2,874 embedded | Read-only DB at 12:53 UTC: 3,430 chunk vectors, 345,272 MiniLM tokens and zero local cost. Collection continued after the timed backfill. |
| Rust coverage fixture | 5.29 MiB logical; no WAL | 461 events; 382 text notes | The former `rust-coverage-8088` file is not active. Its 13,099 indexed tags include 12,167 from kind-3 follow events, so bytes/event is misleading. |
| SDK parity snapshot | 233.43 MiB | 10,001 events; 7,543 projected posts | Follow tables/indexes and full event/tag indexes dominate. It is migration evidence, not a representative monthly storage rate. |
| Rust note-only fixture copy | 0.58 MiB above empty schema | 382 text notes | 1.55 KiB/note for full signed events, event indexes, reader rows, tags and FTS. |

The completed set holds 4,414,464 aggregate-vector bytes and 5,268,480 chunk-vector bytes; every vector is 384 float32 values (1,536 bytes). Embedding tables and their indexes allocate 14,680,064 bytes, or 5,107.9 bytes/completed post. The space records fastembed 6.0.2's `AllMiniLML6V2` model revision plus model and tokenizer hashes. [Measured query and arithmetic](../../slop/verification/2026-09-13_rust-minilm-measured-estimate-evidence.log) preserve the exact values.

## Compute alternatives

- **Bedrock production:** inference is remote, so the EC2 host only chunks, sends and stores results. The existing EC2 role has a narrowly scoped Titan V2 `bedrock:InvokeModel` grant in us-west-2. Application-level role-backed invocation is now proven by the ledgered continuous run, not merely container credential retrieval. The production host is a t3.small with healthy CPU credits at the recorded resource sample, but low free memory/high SQLite I/O and variable public latency: Rust used 1.293 GiB of 1.865 GiB RAM, about 157 MiB was available, the canonical DB was 1.029 GB with a 401.5 MB WAL, and public reader responses ranged from sub-second topic index to multi-second 100-card pages. These are dated observations, not a capacity forecast or a reason alone to upgrade. Continuous production uses the default credential chain rather than profile or static credentials.
- **MiniLM CPU development:** the completed Rust command resumed 1,519 posts in 402.69 s: 3.77 posts/s wall time, 0.157 s CPU/post, 0.59 average cores and 571 MiB maximum RSS on an AMD Ryzen 9 5900X host with 12 cores/24 threads and 62 GiB RAM. This whole command also includes a 14.01 s development build and 11-topic clustering; clustering time is not isolated, and the resumed remainder is a biased subset. At the same desktop whole-command rate, 918,000 posts is 67.6 hours, but that arithmetic is not a t3.small forecast. The earlier Python fastembed path measured 17.4 posts/s on a different four-core setup and is not directly comparable.
- **GPU backfill:** no current Rust/GPU throughput is measured. The original estimate's 5,000–18,000 posts/s range came from an unspecified sentence-transformers benchmark, so it is not adequate evidence to rent hardware. Titan's projected $1.49–$2.21 backfill removes the economic reason to provision a GPU for production.

## Titan API cost

The AWS us-west-2 metered-unit map reports Titan Text Embeddings V2 at $0.00002 per 1,000 input tokens, or **$0.02/million**. The old length sample averaged 81.2 MiniLM tokens/post. The completed Rust set records 345,272 MiniLM tokens / 2,874 posts = 120.14 tokens/post. These give two local-tokenizer projections:

| volume | notes/month | at 81.2 tokens/post | at 120.14 tokens/post |
|---|---:|---:|---:|
| ×0.5 | 459,000 | $0.75 | $1.10 |
| base | 918,000 | $1.49 | $2.21 |
| ×3 | 2,754,000 | $4.47 | $6.62 |

The partial Titan run completed 119 calls before temporary-login refresh failed. The ledger records 7,247 actual Titan tokens and 144,940 nano-USD ($0.00014494), or 60.90 tokens/post. The same 119 IDs use 6,511 MiniLM tokens, so Titan counted 1.113× as many tokens on matched text. The old 81.2-token proxy predicts 9,662.8 tokens for these posts: 33.3% above actual usage; actual usage is 25.0% below the proxy.

This is not a representative correction factor. All 119 Titan events have NIP-13 nonce tags and IDs beginning with four zero hex digits; only 267 and 262 respectively of 6,381 eligible non-Titan notes do. Low-ID ordering selected mined content first. [The exact matched-cohort query](../../slop/verification/2026-09-13_titan119-cost-evidence.md) is application-ledger evidence, not an AWS invoice.

### Larger frozen partial cohort sensitivity

A read-only query of the frozen `events-8136.sqlite` deployment handoff at 2026-09-14 found 9,018 succeeded Titan request rows for 8,136 events: 3,615,029 actual tokens and $0.07230058 actual ledger cost. For comparison with MiniLM, only 7,537 events are both currently reader-eligible and have a completed MiniLM aggregate embedding. Those exact matched IDs used 1,169,047 actual Titan tokens/$0.02338094 and 1,129,414 MiniLM tokens: 155.11 Titan tokens/post and a 1.035 Titan/MiniLM token ratio. Its simple rate sensitivity is:

| volume | notes/month | illustrative Titan input cost |
|---|---:|---:|
| ×0.5 | 459,000 | $1.42 |
| base | 918,000 | $2.85 |
| ×3 | 2,754,000 | $8.54 |

This is deliberately labelled illustrative rather than a replacement forecast. The matched cohort has 930 four-leading-zero IDs among 7,537 events (12.3%), versus 1,010 among 13,874 current eligible posts (7.3%); it is still low-ID/PoW-biased. More importantly, 599 currently eligible successful Titan events lack a MiniLM match and account for 2,445,982 Titan tokens/$0.04891964 (4,083 tokens/event). Excluding them makes the matched sensitivity length-selected as well as low-ID-selected. The ledger also holds three reserved requests with $0.00049152 reserved budget and one uncertain request with 3 actual tokens/$0.00000006; neither is silently treated as completed. [The bounded query and result](../../slop/verification/2026-09-14_titan8136-matched-cost-evidence.md) preserve the denominators. The frozen database is application-ledger evidence, not an AWS invoice.

Initial backfill costs one ingestion-month equivalent under the matching token assumption. Queries, retries, uncertain requests and re-embedding after a model-space change are additional. The user authorized continuous production embedding with a $5 setup/backfill ceiling and a separate $5 monthly ceiling. The setup limit must not become a permanent lifetime block after backfill; recurring calls remain subject to the monthly ledger limit. Mechanically, $5 permits 3.35 ingestion-month equivalents under the 81.2-token proxy, 2.27 under the 120.14-token proxy, or 1.76 under the illustrative matched 155.11-token cohort. The ×3 high-token cases exceed the monthly limit. Full-corpus Titan usage and the AWS invoice remain unavailable.

## Model context

Production is fixed to `amazon.titan-embed-text-v2:0`, 512 dimensions, normalized, in us-west-2. AWS documents 256/512/1024 outputs and reports that 512 dimensions retained about 99% of its own 1024-dimensional retrieval result. This is an AWS self-report across its evaluation, not evidence on Meaty Broth posts.

Local MiniLM has strong adoption evidence: the Hugging Face API reported 252,928,721 downloads and 5,907 likes when fetched. Downloads do not measure retrieval quality. MTEB retrieval tasks are better comparison context than its aggregate rank, but a local short-social-text check is still required.

Larger local stacks are possible but not free substitutions. Ollama listed the 300M-parameter EmbeddingGemma package at 622 MB and 2.1M downloads. QMD uses a quantized ~300 MB embedder plus a ~640 MB reranker and ~1.1 GB query-expansion model. That three-model design solves a broader desktop-search problem; it does not justify increasing this service's EC2 memory or model count.

[xAI Phoenix](https://github.com/xai-org/x-algorithm/blob/main/phoenix/README.md) is useful algorithm context, not a compatible embedding model. It learns user and candidate towers and uses residual-quantized semantic IDs derived from multimodal embeddings. Its quoted 2,560-dimensional ranking configuration is an internal representation, so comparing its width with Titan's 512 output says nothing about quality.

[Source excerpts and epistemic context](../../slop/verification/2026-09-13_embedding-source-evidence.md) preserve the AWS, Hugging Face, Ollama, QMD, MTEB and Phoenix references.

-- Pi/gpt-5.6-sol
