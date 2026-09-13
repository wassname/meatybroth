# Titan 119-call cost evidence

Observed 2026-09-13 by read-only queries of `.local/rust-live-8086/events.sqlite`. No new Bedrock call was made for this check.

Titan space `64c3c581f1da5a1973fb22e9bae7d91988c2298f14b92cbc3f89f1019703bbf5`:

- 119 post vectors and 119 chunks
- 7,247 recorded actual input tokens
- 144,940 nano-USD recorded actual cost = $0.00014494
- 60.90 Titan tokens/post
- 119 × 2,048 = 243,712 raw chunk-vector bytes, plus the same aggregate-vector bytes

Matched MiniLM comparison on the exact same 119 event IDs:

- 6,511 MiniLM tokens in 120 chunks
- 54.71 MiniLM tokens/post
- Titan/MiniLM token ratio: 7,247 / 6,511 = 1.113

The old 81.2-token proxy predicts 9,662.8 tokens and $0.000193256 for 119 posts. The proxy is 33.3% above actual usage; actual usage is 25.0% below the proxy. This is not a calibration of the retained corpus: all 119 Titan events have NIP-13 nonce tags and IDs beginning with four zero hex digits because `ORDER BY event_id` selected proof-of-work notes first. Among 6,381 eligible non-Titan notes, only 267 have nonce tags and 262 begin with four zero hex digits.

The database ledger is application evidence, not an AWS invoice. Account billing was not fetched. The user authorized only the one-off run with a $5 total ceiling, not recurring monthly Bedrock spending. Full-corpus Titan cost and production-host throughput remain unmeasured.

Read-only SQL result:

```text
titan_posts=119 titan_tokens=7247 titan_cost_nusd=144940
matched_minilm_posts=119 minilm_tokens=6511 minilm_chunks=120
```

-- Pi/gpt-5.6-sol
