# Maintained source reduction audit

Date: 2026-09-13. Read-only review of `e1c8fbe89dcd62eb67792d1460fcbc5eff72ac0b`.

## Count result

The current Rust implementation is not smaller than either the true pre-SDK Python application or the later mixed Python/Rust reference. The same counter and boundaries were used on all commits: raw source lines including blanks/comments; runtime, SQL, maintained operational scripts, templates/CSS, and deployment are production; tests are separate; generated files, research evidence, lockfiles, and vendored stopword data are excluded.

| snapshot | production lines↓ | production files↓ | runtime + SQL↓ | tests↓ | note |
|---|---:|---:|---:|---:|:---|
| *Pre-SDK Python `f9c297820a6d`* | **3,416** | 22 | **2,094** | 2,774 | Parent of the isolated SDK collector scaffold; the closest true pre-migration commit. |
| Mixed Python/Rust `342085089a8e` | 4,712 | 27 | 3,214 | 3,382 | Already includes the separate 1,106-line Rust SDK collector. |
| Rust `e1c8fbe89dcd` | 5,617 | **20** | 4,919 | **1,680** | Excludes 1,321 lines of vendored stopword data on the same non-runtime basis. |

Against the true pre-SDK application, current production source is 2,201 lines (+64.4%) larger; application runtime plus SQL is 2,825 lines larger. Against the later mixed reference it is 905 production lines (+19.2%) larger. File count and tests decreased, but those reductions do not offset runtime growth. There is no supported whole-application source-reduction acceptance claim.

`slop/verification/reader-source-counts.json` is a narrower reader milestone, not a migration baseline: it counts only `main.rs`, `queries.rs`, `render.rs`, templates/CSS, tests, two verification scripts, Cargo metadata, and the README. It excludes collection, embeddings, all runtime SQL, and deployment. Its `1,224` production figure therefore cannot support a whole-application reduction claim.

Much of the growth implements functionality absent from the earlier reader slice: one-file SDK collection, EOSE/coverage records, signed moderation snapshots, two embedding backends, durable request/cost accounting, continuous scheduling, semantic search, and topics. This explains scope, not a source reduction.

Evidence: [`source-count-pre-sdk-f9c2978.json`](source-count-pre-sdk-f9c2978.json), [`source-count-e1c8fbe.json`](source-count-e1c8fbe.json) (SHA-256 `22285e4088c5fb7eeae736937b18bfe59d9bce78a139beeac7015d37c18a5c04`), and [`reader-source-counts.json`](../../verification/reader-source-counts.json) (SHA-256 `d3dba3aa5a6d44979c160a0decd0356af8b3699c5a0e2d6bae931951b4a84166`).

## Small reductions supported by the source

These estimates are net ranges, not achieved counts. The first three total 45–63 production lines. Including the conditional fourth totals 65–93, far less than either measured difference.

| candidate | likely lines↓ | exact source | behavior that must remain | discriminating maintained test |
|---|---:|:---|:---|:---|
| Merge completed-run finalization | **20–25** | `src/collect.rs:128-197`, `finish_run` and `finish_unreconciled` | Run completion, optional unreconciled gap insertion, and cursor update remain one transaction; interrupted scans never advance the cursor. | `collect::tests::coverage_cursor_survives_restart_and_interruption_becomes_gap`; `sdk_tests::eose_boundary_has_no_late_admitted_write` |
| Use Nostr SDK tag iterators | **15–22** | `src/collect.rs:655-713`, manual `p`/`e` tag parsing in `missing_direct_follow_contact_lists` and `hydrate_notes` | Only valid NIP-02 public keys and event IDs are hydrated; newest stored contact list still defines missing owners. `nostr 0.45.4` already provides `Tags::public_keys()` and `Tags::event_ids()`. | `tests::social_uses_canonical_graph_shortest_independent_paths_and_reach`; SDK relay-to-reader and context assertions in `sdk_tests::sdk_relay_to_atomic_fts_http_policy_and_expiry` |
| Share ranked-ID card hydration | **10–16** | `src/main.rs:285-381`, repeated `eligible_map` lookup loops in `semantic_feed` and `topic_feed`; `canonical_event_id` at 263-271 | Preserve ranking order, page offset, score values, topic membership, and omission of ineligible/stale IDs. | Semantic/Similar/Topics assertions in `sdk_tests::incremental_embeddings_reuse_delete_and_budget_after_sdk_drain`; `sdk_tests::machine_presence_envelopes_stay_auditable_but_not_reader_or_embedding_eligible` |
| Retire the old table-shaped warning reader only after cutover | **20–30** | `src/render.rs:242-304`, `sqlite_master` branch in `warning_map` | Do this only when old/reference databases no longer need direct read-only inspection. Canonical `content_warnings` view, signed spam list, NIP-36 warnings, duplicate flags, and hide policy must remain. | `tests::rendering_preserves_safe_text_profiles_warnings_and_exclusions`; `tests::matched_reference_reader_outputs`; public read-only fixture test if legacy support is intentionally removed |

The first three are low-risk consolidation or existing-SDK reuse. The fourth is conditional feature retirement, not a current safe deletion.

## Changes not recommended

- Do not remove EOSE reconciliation, cursor/gap transactions, collection deadlines, embedding request ledger, signed-event verification, moderation admission, or stale-vector cleanup. Each protects a failure already reproduced by maintained tests.
- Do not replace the proprietary Primal-cache WebSocket with a standard Nostr SDK request: its `cache` request is not a standard relay subscription.
- Do not remove batch identity/warning/parent caches or split the large feed SQL merely to lower line count. The former prevent per-card scans; the latter would reorganize rather than reduce behavior.
- Do not count moving verification Python or vendored data outside this repository as runtime reduction.

## Evidence ownership

The text reports and count JSON under `slop/reviews/live-reader-review/` are currently ignored. If this audit is intended to remain acceptance evidence, an owner should add the small text/JSON artifacts explicitly. The 41 MB selective Titan snapshot and PNG set should remain uncommitted binary evidence.

-- Pi/gpt-5.6-sol
