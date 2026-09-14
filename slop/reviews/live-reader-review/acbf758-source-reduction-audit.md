# acbf758 maintained-source reduction audit

Read-only comparison:

- true pre-SDK Python application: `f9c297820a6d7ad1442ba168e258a22f3236e25f` in `/workspace/meatybroth`
- current Rust application checkpoint: `acbf75874d1627f256070f061f7a1ff67a718aea` in `/workspace/.worktrees/meatybroth-sla`

I used the same raw-line boundaries as the committed pre-SDK artifact [`source-count-pre-sdk-f9c2978.json`](source-count-pre-sdk-f9c2978.json): production is application runtime/SQL, templates/CSS, maintained deployment and operational scripts; tests are separate; lockfiles, docs, `slop`, generated output, and vendored stopwords are excluded. For each tracked qualifying file, the counting command was `git -C REPO show SHA:PATH | wc -l`; paths came from `git -C REPO ls-tree -r --name-only SHA`. The current categories are `src/*.rs`, `src/*.sql`, `templates/*`, `static/style.css`, and `infrastructure/deployment/*`; `src/{sdk_tests,tests}.rs` are tests and `src/data/*` is vendored data.

| category | f9c2978 lines | acbf758 lines | change |
|:---|---:|---:|---:|
| application runtime + SQL | 2,094 | 6,122 | +4,028 |
| templates/CSS | 351 | 396 | +45 |
| deployment | 555 | 308 | -247 |
| maintained scripts | 416 | 0 | -416 |
| **production total** | **3,416** | **6,826** | **+3,410 (+99.8%)** |
| production files | 22 | 20 | -2 |
| tests, separate | 2,774 | 2,784 | +10 |

The requirement is still unmet. The old Python scripts and deployment material are gone, but the added Rust runtime/SQL is substantially larger. This is not an argument to delete agreed features: the current side adds SDK relay collection and reconciliation, moderation snapshots, profile/parent hydration, shared SQLite reader projections, two embedding spaces and accounting, semantic/Similar results, both clustering algorithms, topic assignment, and their functional regressions. Those are product scope that did not exist in the pre-SDK baseline, not a source reduction.

## Concrete reductions that preserve scope

| candidate | estimated net lines | source | behavior that must remain | discriminating verification |
|:---|---:|:---|:---|:---|
| Share collection-run finalization | 20–25 | `src/collect.rs:142-222`, `finish_run` and `finish_unreconciled` repeat connection, observation update, cursor update, and transaction setup | Reconciled scans finish without a gap; unreconciled scans write one gap; both persist the same cursor atomically. | `collect::tests::coverage_cursor_survives_restart_and_interruption_becomes_gap`; EOSE late-write regression. |
| Use maintained Nostr `Tags` accessors | 15–22 | `src/collect.rs:306-316`, `679-689`, `716-727` manually inspect tag slices and parse `p`/`e` values | Only valid NIP-02 `p` keys and event-ID parents are requested; malformed tags do not turn into identities. | Direct-follow contact-list hydration; reply-context and Social graph regressions. |
| Share cluster input loading | 20–30 | `src/embed.rs:1232-1255` and `1415-1434` each query the same selected-space 30-day `(event_id, vector, content)` rows and decode all vectors before K-means or DBSCAN | K-means and DBSCAN use the identical eligible vector/text set for a fixed space/time but retain their distinct algorithms and noise behavior. | Existing fixed-k/DBSCAN membership and stateful topic tests; verify selected-space IDs still match each membership table. |

The first two reuse existing functionality; the third removes duplicated SQL/decoding only. A likely total is 55–77 lines, not the 3,410-line gap. Do not remove collection gaps, EOSE reconciliation, moderation admission, request ledger states, stale-derived cleanup, model provenance, or topic/Similarity diagnostics merely to lower the number. Do not count moving code, compressing formatting, or excluding maintained files as a reduction.

-- Pi/gpt-5.6-terra
