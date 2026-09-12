# Niche-search evaluation set (hand-labelled, frozen)

> Status: scaffold — not yet populated. This is the project's scoreboard.
> Per SPEC.md, the gold set is frozen (queries + corpus snapshot), and the
> rejection regime is the conditional funnel:
> `P(ingestion) → P(topology | ingestion) → P(candidacy | topology)
>  → P(rank ≤ 10 | candidacy)`.
> One half stays held-out and is never tuned on; refresh half the set monthly
> from newly observed threads. Include strata where naive FTS5 should win
> (negative control) so the set falsifies rather than confirms.

## Query list (seed)

| Query | Why it matters | Target replies (source, id) | In-corpus? | Root correct? | In top-10? |
|---|---|---|---|---|---|
| `speechmap.ai` | brand/niche | | | | |
| `activation steering` | niche alignment topic | | | | |
| `steering activations` | near-duplicate phrasing | | | | |
| `representation engineering` | niche alignment topic | | | | |
| `gradient routing` | niche alignment topic | | | | |
| `J-Lens` | obscure | | | | |
| (specific alignment paper) | TBD | | | | |

Per-query questions (SPEC.md): relevant recent posts? replies buried in
threads? people outside the follow graph? bridged content? bot spam dominating?
explainable ranking? best conversations in the first ~10?

## Methodology

- Target replies are >2 levels deep and/or jargon-dense; author from each
  source's native API/UI, not the ingest pipeline.
- Corpus snapshot ID + hydration state recorded per run.
- Metrics: recall@10, MRR, with Wilson CIs.
- Baseline: pure FTS5, unweighted (the "ordinary Nostr search" comparator).
