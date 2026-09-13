# Rust reader verification

Pi/gpt-6-astra, 2026-09-13. This is the delegated reader slice, not completion of the Rust application replacement.

## Open

- [Rust, matched snapshot](http://localhost:8083/?q=bitcoin&mode=relevance)
- [Python reference, isolated matched snapshot](http://localhost:8084/?q=bitcoin&mode=relevance)
- [Rust, original SDK database read-only](http://localhost:8085/?mode=discovery&reach=3&order=connections)
- [Paired screenshots and actual browser actions](reader-browser/observations.json): `reader-browser/{before,after}-{search,mobile,social,context}.png`.

The original repository and databases were not edited. Port 8082 had no listener when checked; the Python comparison on 8084 uses a copy in this sibling. Each Rust process reads one selected SQLite file. These comparison snapshots are verification inputs, not a production export pipeline.

## Evidence

- `reader-restored-nightly-tests.log`: five maintained HTTP regressions pass under the repository-selected nightly compiler. They cover canonical profiles and follow metadata, pagination/URL actions, literal search parsing, all four feeds, ancestry/descendants/cycles, warnings, nested images, escaping, expiry, missing routes and moderation status. A benign same-topic post remains visible when an NSFW-listed author's post is removed from both search and context.
- `reader-parity-{expected,observed}.json` and `reader-restored-parity.log`: 46 nonempty original-handler cases have identical ordered IDs with 6,536 common posts and 251 signed profile/follow events.
- `reader-fullgraph-{expected,observed}.json` and `reader-restored-fullgraph-parity.log`: another 46 nonempty cases compare the same posts with 863 actual SDK follow lists (1,060 profile/follow events total). Both matrices include all reaches 1–3, recent/connections order, query filtering and pagination. Rust fixtures have neither `nostr_state` nor the old `follows` table.
- `reader-browser.log` and the action JSON: actual Go/select/reach/order submissions, page two with 50 cards, expansion/collapse, mobile width, context and status/terms routes succeed. Malformed search returns 400; missing context returns 404. No image/media/script elements appear in post cards.
- `reader-restored-build.log`: the restored code builds with repository-selected nightly Cargo. Earlier strict all-target clippy passed. `cargo-age-hold.log` proves resolution reports `as of 8 days ago`; `.cargo/config.toml` makes the eight-day hold repository-local. Cargo 1.100 stabilized this setting. The obsolete global unstable flag warning is separate; the supervisor is handling that dotfile.

Post fields and metadata are deliberately held equal in the paired databases to isolate reader behavior. This does not establish collection, migration completeness or signed-event admission correctness. Scripts under `slop/verification/` use the existing reference Python environment only to generate expectations and drive the browser; the application has no Python dependency.

## Bugs found during verification

The first real-corpus comparison passed 43/46 cases. In the three Conversations failures, SQL excluded a root author's replies even when that root belonged to another stored thread grouping. Restricting root-author lookup to the same group restores the original definition (`meatybroth/ranking.py:134–146`). The failing ordered IDs remain in `reader-parity-before-fix.json`; a maintained HTTP regression detects this case.

Nested Markdown images initially caused a renderer panic. `nested-image-before-fix.log` records the actual HTTP-handler failure; the parser now collects nested image alt text without rendering media.

Social initially expanded paths to authors without eligible posts. `reader-timing.json` records the slow version. Restricting only endpoints preserves all intermediates and first-hop endorsers. Materialized endpoint edges avoid repeating irrelevant joins. Scores use the original count-based arithmetic, retaining indirect support for direct follows and the shortest path per distinct endorser.

An optional SDK `event_tags` index experiment was discarded: although IDs still matched, the full-graph test took 425 seconds (`reader-native-index-parity.log`). Its plan chose a tag-led primary-key lookup per event (`social-native-index-plan.txt`); the measured result did not justify keeping it. Restored JSON graph extraction plus endpoint filtering is the delivered code. No new persisted graph table or cache was added. The reader still depends on the existing post/FTS projection; replacing that storage design belongs to collection integration.

`reader-timing-final.json` records checked HTTP responses from the delivered endpoint-filter approach: matched Social Rust 0.59–1.00 seconds versus Python 0.59–0.98 seconds; the larger actual SDK graph took 3.50–4.85 seconds. This remaining latency is explicit, not a claim of production scalability. All measured feed responses returned 200 with 50 actual IDs; no stale response body was counted after a timeout.

Source before final config/docs changes: 869 Rust production lines, 440 Rust test lines, 356 template/CSS lines (1,225 production lines including templates/CSS). Verification scripts are separate from runtime. The original Python application remains preserved, so this is not a claim of repository-wide line reduction.

## Remaining scope

No collection, schema creation, retention cleanup, embeddings, clusters, cloud calls, deployment or removal of the original Python runtime was done. The comparison notice remains visible for that reason. Existing missing emoji glyphs remain an environment limitation. CommonMark rendering is not byte-for-byte Python-Markdown output; the inspected cards, safe links and expansion behavior are preserved.

The supervisor independently inspected all eight final screenshots and reported preserved layout, wrapping, names/NIP-05, scores, warnings and nested replies, apart from the comparison notice (Intercom message `07e1085e-1e36-4687-9b41-c15feeb5d390`).

Reproduction commands are in `README.md`. For the paired checks, run `prepare_reader_parity.py` with the existing reference interpreter, then set `MEATYBROTH_PARITY` to the generated sibling `.local/reader-parity` directory and run `cargo test --locked matched_reference_reader_outputs -- --ignored`. Repeat preparation with `--sdk-graph` and point the test at `.local/reader-fullgraph` for the larger graph. `browser_reader.py` provides the isolated reference server, browser capture and checked HTTP measurement commands.
