# Embedding production estimates

Date: 2026-09-13. This updates the original Python-era estimate for the Rust SQLite schema. Prices are USD.

## Answer

At the observed base rate, one retained month is **918,000 text notes**. Titan V2 API input is about **$1.49/month** if Titan counts the same 81.2 tokens/note as the MiniLM length sample. That token equivalence is not measured; the first authorized backfill must replace it with Titan's returned `inputTextTokenCount`.

The current schema stores each chunk vector and one aggregate vector per post. SQLite allocation is therefore materially larger than `notes × dimensions × 4`:

| space | retained DB↓ | +25% working room↓ | API/month↓ | use |
|---|---:|---:|---:|---|
| *MiniLM 384-fp32* | *5.62 GiB* | *7.02 GiB* | *$0* | local development only |
| Titan V2 512-fp32 | 8.80 GiB | 11.00 GiB | ~$1.49 | production after approval |

These totals include the measured Rust note/event/index/FTS density and synthetic allocation of the exact embedding tables. They exclude profiles, follows, collection history, backups, model cache and a future ANN index. The current 30 GiB volume is probably enough for one month. Bedrock runs inference outside EC2, so embeddings alone do not justify a larger instance. Production web/collection CPU and RAM are still unmeasured, so this does not establish that the whole service needs no upgrade.

## Inputs and arithmetic

The volume source is the NostrMash snapshot preserved in the original repository: 30,600 text notes/day from 12 active tracked relays. It is one indexer's coverage, not all Nostr.

- Retained notes: $30{,}600\;\text{notes/day} \times 30\;\text{days} = 918{,}000\;\text{notes}$.
- Sensitivity: 459,000 at ×0.5; 2,754,000 at ×3.
- Length sample: 7,937 non-empty posts, mean 81.2 MiniLM tokens/post and 2,334,145 UTF-8 bytes total.
- MiniLM chunking: 9,279 chunks / 7,939 posts = 1.169 chunks/post from the earlier measured run.
- Titan chunking: the current 8,000-byte Rust splitter produces 7,982 chunks / 7,937 non-empty sampled posts = 1.0057 chunks/post; 17 posts need more than one chunk.

Raw float32 vector payload is:

$$S_{raw} = N_{posts} \cdot (1 + c_{chunks/post}) \cdot d_{dimensions} \cdot 4\;\text{bytes}$$

where the `1` is the aggregate post vector retained in addition to chunk vectors. This gives 2.85 GiB for MiniLM and 3.51 GiB for Titan. A 10,000-post synthetic fill of the exact SQLite schema measured 4.26 GiB and 7.44 GiB after scaling to 918,000 posts. Titan's 512-float rows cross a SQLite overflow-page boundary with the current 4 KiB page size, which explains why allocated storage grows by more than the 512/384 dimension ratio.

The non-embedding Rust note sample measured 1,586.9 bytes/note after subtracting the empty schema. Scaling this event, tag-index, reader-row and FTS footprint adds 1.36 GiB. The 25% column allows for WAL, fragmentation and ordinary growth; it is not a substitute for the excluded metadata measurements.

The request ledger does not expire with posts. Its synthetic allocation grows by about 0.25 GiB/month for MiniLM or 0.22 GiB/month for Titan at base volume. For Titan that is about 2.6 GiB/year even though vectors retain only 30 days. This ledger must survive host replacement because it is also the evidence used by the cumulative spending limit.

## What is observed today

Observed 2026-09-13; [full SQLite output](../../slop/verification/2026-09-13_embedding-storage-observations.log).

| database | file state | rows | interpretation |
|---|---:|---:|---|
| Original reader | 27.18 MiB | 7,937 posts | Selected 30-day corpus; post/FTS density was previously measured at 1.36 KiB/post. |
| Rust current 8088 | 5.29 MiB logical; no WAL at observation | 461 events; 382 text notes | No embedding tables yet. Its 13,099 indexed tags include 12,167 from kind-3 follow events, so bytes/event is misleading. |
| SDK parity snapshot | 233.43 MiB | 10,001 events; 7,543 projected posts | Follow tables/indexes and full event/tag indexes dominate. It is migration evidence, not a representative monthly storage rate. |
| Rust note-only copy | 0.58 MiB above empty schema | 382 text notes | 1.55 KiB/note for full signed events, event indexes, reader rows, tags and FTS. |

The current databases contain zero `embedding_spaces`, `embedding_chunks` or `post_embeddings` rows. Persisted vector bytes and Rust MiniLM throughput are therefore **pending**, not inferred from the earlier Python fastembed run. The app owner selected fastembed 6.0.2 with native ONNX Runtime and `AllMiniLML6V2`; their measured hardware, corpus, wall time and persisted bytes must be added after the real backfill.

## Compute alternatives

- **Bedrock production:** inference is remote, so the EC2 host only chunks, sends and stores results. The API price and storage are the relevant embedding increments; web and collection load still need a representative host measurement.
- **MiniLM CPU development:** the earlier Python fastembed path measured 17.4 posts/s and 641 ms process CPU/post on a four-core slice of a Ryzen 9 5900X with production chunking. It exceeded the 0.354 notes/s base arrival rate on that desktop, but it is not a Rust or t3.small measurement. The current Rust result is pending.
- **GPU backfill:** no current Rust/GPU throughput is measured. The original estimate's 5,000–18,000 posts/s range came from an unspecified sentence-transformers benchmark, so it is not adequate evidence to rent hardware. Titan's projected $1.49 backfill removes the economic reason to provision a GPU for production.

## Titan API cost

The AWS us-west-2 metered-unit map reports Titan Text Embeddings V2 at $0.00002 per 1,000 input tokens, or **$0.02/million**. Under the provisional MiniLM-token assumption:

| volume | notes/month | assumed tokens/month | Titan input/month |
|---|---:|---:|---:|
| ×0.5 | 459,000 | 37.27M | $0.75 |
| base | 918,000 | 74.54M | $1.49 |
| ×3 | 2,754,000 | 223.62M | $4.47 |

Initial backfill costs the same as one retained month. Queries, retries, uncertain requests and re-embedding after a model-space change are additional. The configured $5 setup and $5 monthly limits leave room at base volume, but ×3 leaves only $0.53 before those extras. No Bedrock call has been made, and these figures are not an AWS bill.

## Model context

Production is fixed to `amazon.titan-embed-text-v2:0`, 512 dimensions, normalized, in us-west-2. AWS documents 256/512/1024 outputs and reports that 512 dimensions retained about 99% of its own 1024-dimensional retrieval result. This is an AWS self-report across its evaluation, not evidence on Meaty Broth posts.

Local MiniLM has strong adoption evidence: the Hugging Face API reported 252,928,721 downloads and 5,907 likes when fetched. Downloads do not measure retrieval quality. MTEB retrieval tasks are better comparison context than its aggregate rank, but a local short-social-text check is still required.

Larger local stacks are possible but not free substitutions. Ollama listed the 300M-parameter EmbeddingGemma package at 622 MB and 2.1M downloads. QMD uses a quantized ~300 MB embedder plus a ~640 MB reranker and ~1.1 GB query-expansion model. That three-model design solves a broader desktop-search problem; it does not justify increasing this service's EC2 memory or model count.

[xAI Phoenix](https://github.com/xai-org/x-algorithm/blob/main/phoenix/README.md) is useful algorithm context, not a compatible embedding model. It learns user and candidate towers and uses residual-quantized semantic IDs derived from multimodal embeddings. Its quoted 2,560-dimensional ranking configuration is an internal representation, so comparing its width with Titan's 512 output says nothing about quality.

[Source excerpts and epistemic context](../../slop/verification/2026-09-13_embedding-source-evidence.md) preserve the AWS, Hugging Face, Ollama, QMD, MTEB and Phoenix references.

-- Pi/gpt-5.6-sol
