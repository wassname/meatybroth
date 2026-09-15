# Integrated local MiniLM result

Source: `9932c7f5846d816ecaa093d245b7c5811007297d`

Persistent local state:

- database: `.local/integrated/events.sqlite` (about 540 MB plus WAL)
- model: `.local/integrated/models/minilm/` (about 87 MB)
- active reader/writer: `http://localhost:8088/`
- process: `integrated-minilm-writer-8088`

Public AWS readers were not changed. The process has `MEATYBROTH_EMBED_BACKEND=minilm` and no Bedrock/AWS substitution.

## Observation: collection → eligibility → local vector → between-rebuild topic

Event `093818c09fb6ea0953ef691b98ccb53858e8124c22e35db635e19448950df7c3` was absent before the writer start and then received at epoch `1789359637`. It joined `reader_post_events`, so it passed reader eligibility. Its content starts:

> Agent discovery on the open internet still relies on crawling and assumptions. It’s a bootstrap problem for the agentic web. agent_card solves this with signed, self-hosted Agent Cards...

The MiniLM process embedded it at `1789359643`. It received fixed-k topic 5 and DBSCAN topic 18 (`is_core=0`, assigned at `1789359643`). Both topic systems' latest rebuild timestamps remained `1789355552`, 4,091 seconds before embedding. This is evidence of assignment between rebuilds rather than a coincident rebuild.

## Observation: ordinary reader responsiveness

Twenty New requests while the collector was active all returned HTTP 200 in 0.041–0.078 seconds. Twenty more immediately after a MiniLM between-scan embedding log all returned HTTP 200 in 0.043–0.053 seconds. This local sample does not predict public EC2 latency.

## Observation: nonliteral Meaning

Query:

> digital coworkers staying on task

The first result contains none of those query words and starts:

> OpenClaw proved AI agents can be great demos. It also proved how quickly things break when you need them to run reliably as real production systems. With ModelCat you define your agents in a simple YAML config. They run 24/7 ... with structured state, hard failure boundaries, and audit trails...

This is a direct semantic match to reliable autonomous software rather than a literal term match. The request returned HTTP 200 in 0.31 seconds before restart and 0.40 seconds after restart. The rendered selector said `MiniLM`.

## Observation: restart reuse

Before restart the selected event had one MiniLM vector:

- `embedded_at=1789359643`
- `input_tokens=180`
- `chunk_count=1`
- `vector_bytes=1536`
- SHA3-256 `1eb22aece15eb00aa54d4384050bc63b086c7b94b6d326fc55154ba649beaf0d`

After restart those values were identical and there was still exactly one row. MiniLM setup fell from 9,561 ms on the initial model setup to 456 ms on the final port-8088 restart. The final database `PRAGMA quick_check` returned `ok`.

## Qualifications

- The relay.damus.io cycle reported recoverable connection failures, while Primal and purplepag.es completed hydration. The writer continued and the HTTP reader remained available.
- Many pending eligible posts from the copied snapshot were also embedded during the observation window. The selected proof event is stronger because its `received_at` is after writer start.
- Meaning quality is one qualitative example, not an aggregate retrieval evaluation.

## Evidence

- `slop/audits/2026-09-14_integrated-minilm-plan.md`
- `slop/verification/2026-09-14_integrated-minilm-before.log`
- `slop/verification/2026-09-14_integrated-minilm-writer.log`
- `slop/verification/2026-09-14_integrated-minilm-new-event-assignment.log`
- `slop/verification/2026-09-14_integrated-minilm-reader-during.log`
- `slop/verification/2026-09-14_integrated-minilm-reader-after-embed.log`
- `slop/verification/2026-09-14_integrated-minilm-meaning-2-results.log`
- `slop/verification/2026-09-14_integrated-minilm-pre-restart.log`
- `slop/verification/2026-09-14_integrated-minilm-post-restart.log`
- `slop/verification/2026-09-14_integrated-minilm-final-8088.log`
- `slop/verification/2026-09-14_integrated-minilm-live-startup.log`

-- Pi/gpt-5.6-sol
