# Source accounting after embeddings

Read-only count on 2026-09-13. This preserves, rather than replaces, the earlier audit at `/workspace/meatybroth/slop/reviews/2026-09-13_source-accounting-review.md`.

## Pinned snapshots

- Original application: `/workspace/meatybroth` at `342085089a8ee770109e0ea9feb49a38968a759e`
- Rust sibling: `/workspace/meatybroth-rust` at `3ea764bf3203781741bcebdd0301b63804e21f2b`

This is not a true pre-migration baseline: `3420850` already contains the SDK collector migration. The original snapshot is no longer purely Python, and I counted its maintained collector Rust so that moving responsibility between repositories cannot create an apparent reduction. The earlier audit linked above preserves its narrower pre-migration baseline. Uncommitted owner edits in both worktrees are excluded by the commit pins.

## Raw line breakdown

Original maintained production: 4,712 lines.

- Python runtime: 2,108
- SDK collector Rust runtime: 1,106
- maintained benchmark, embed and development scripts: 592
- HTML/CSS: 351
- deployment YAML, shell, Caddy and container files: 555
- maintained tests, separate: 3,382

Rust sibling maintained production: 5,123 lines.

- Rust runtime: 4,126
- runtime SQL: 310
- maintained scripts: 0
- HTML/CSS: 379
- deployment YAML: 308
- maintained tests, separate: 1,278
- vendored stopword data and its provenance files, excluded: 1,321

The replacement comparison is **+411 maintained production lines (+8.7%)**, not a reduction. Production plus maintained tests is 8,094 before and 6,401 after, a reduction of 1,693 lines, but that comes from 2,104 fewer test lines and does not make the maintained runtime smaller.

The old implementation has not been removed. At these two pins, both repositories together retain at least 9,835 maintained production lines. Therefore neither the test reduction nor the sibling comparison proves the user’s “less lines” constraint is achieved.

## Scope

Included symmetrically where present: runtime Python/Rust, runtime SQL, maintained operational scripts, templates/static CSS, and deployment source. Rust `#[cfg(test)]` modules and dedicated test files are counted as tests, not runtime.

Excluded on both sides: README and research documentation, verification/slop artifacts, manifests and lockfiles, generated build output, local databases/models, and vendored data. Blank lines and comments remain in the raw counts. This is a size census, not a complexity metric.

## Obvious retained duplicates or replacement candidates

- The original Python runtime (`ingest.py`, `store.py`, `ranking.py`, `web.py`) remains beside the Rust runtime that now owns those responsibilities.
- `scripts/embed_corpus.py` remains beside `src/embed.rs` and `src/embed.sql`.
- The original SDK collector remains beside `src/collect.rs`.
- `_post.html`, `about.html`, and `context.html` are exact copies at the two pins. `base.html`, `feed.html`, `status.html`, `tos.html`, and `style.css` are close descendants; see `template-diff-stat.txt`.
- Two deployment definitions remain: the original 555-line Caddy/container/CloudFormation/shell set and the sibling’s 308-line deployment template.

These are candidates for removal only after the sibling is accepted as canonical. Their current presence must count against any claim that the migration already reduced source.

## Simplification candidates after functionality stabilizes

These are review proposals, not achieved reductions. They do not change the pinned source comparison above.

1. Put dynamic reader eligibility in the existing `reader_post_events` view, then remove repeated policy/NSFW clauses from `src/queries.rs` and five queries in `src/embed.rs` (`pending`, `cache_status`, `cluster_topics`, `topic_events`, `nearest`). `posts` already joins this view, so the view is the natural current source of truth rather than a new layer. Rough reduction: 45–55 production lines after retaining the time-window and vector-space predicates. This would make dynamic policy and NSFW changes consistent across text search, embedding work, topics and status. The prior stale-vector defect also required its separate derived-data cleanup and Similar source-eligibility check; this proposal does not replace either. Guard behavior with existing `machine_presence_envelopes_stay_auditable_but_not_reader_or_embedding_eligible`, `sdk_relay_to_atomic_fts_http_policy_and_expiry`, `rendering_preserves_safe_text_profiles_warnings_and_exclusions`, and the cache-status assertions.

2. Remove the unused multi-source field from reader cards. `posts.sql` always emits `source='nostr'`, but `queries::Post`, `post()`, conversation author distinctness, `render.rs`, and `_post.html` carry it through every card. Keep `source_id` and `canonical_id`; use the literal `nostr` only where the context URL is rendered. Rough reduction: 8–12 production lines plus fixture columns. Guard behavior with `matched_reference_reader_outputs`, `pages_query_state_and_invalid_requests_use_real_handlers`, `conversation_context_counts_cycles_and_warning_excerpts`, and the SDK HTTP policy test. This is appropriate only while the accepted product remains Nostr-only.

3. Resolve the selected embedding space once per request in `main.rs::feed` and pass it to `semantic_feed` / `topic_feed`. The current Titan path can call `selected_space` for the vector count, topics, and semantic ranking; each call reopens SQLite through `space_by_backend`. One resolved `Result<Space>` can supply all three without changing the MiniLM/Titan separation or error text. Rough reduction: 8–15 production lines and up to two read-only database opens per request. Guard behavior with `pages_query_state_and_invalid_requests_use_real_handlers`, `incremental_embeddings_reuse_delete_and_budget_after_sdk_drain`, and the SDK test assertions for Meaning, Similar, Topics, explicit uncached Titan errors and model labels.

I did not recommend removing comments, tests, Titan accounting, read-only inspection, the two MiniLM instances, or the direct/batch rendering paths. Each currently carries a documented behavior, cost boundary, latency property, or test seam; deleting it would be line counting rather than demonstrated simplification.

## Evidence

- `source-count.json`: per-file categories and raw counts
- `/workspace/meatybroth/slop/reviews/source-accounting/count_source.py`: temporary reproducible Git-object counter, outside the sibling Rust repository
- `template-diff-stat.txt`: exact/near-copy template comparison

-- Pi/gpt-5.6-sol
