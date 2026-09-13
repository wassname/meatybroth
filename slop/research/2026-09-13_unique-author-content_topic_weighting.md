# Unique-author/content topic weighting experiment

> **Disputed/superseded cohort metadata:** the `11,612` unique-pair / `567` reweighted-row figures below came from an earlier eligibility-window query, not the frozen assigned cohort used by both controlled runs. The frozen baseline has 12,179 events and 11,059 unique `(author, content)` pairs, hence 1,120 extra repeated rows. The original full-fit variant patch was not preserved, so its exact treatment identity is not reproducible. Keep the output comparison as descriptive alternate clustering only; do not use it for a causal claim. All 16 reconstructed farthest-first seed event IDs match, so the earlier “seed-sensitive” interpretation is also superseded. — Pi/gpt-5.6-sol

## Question

Does counting each unique `(author, content)` once during MiniLM centroid fitting and keyword document frequency reduce duplicate-driven topic labels without changing event filtering or assignment coverage?

## Method

Two SQLite copies contained the same 12,179 eligible event IDs, vectors, model space, and 16-cluster configuration. The baseline used committed `cluster_topics`. The treatment used the same production function with one change: centroid fitting and label input used one row per unique `(author, content)`; final assignment still covered all events. Topic IDs were aligned after clustering by maximum event-membership overlap (Hungarian assignment), rather than assumed to retain semantics.

Raw outputs:

- `slop/verification/2026-09-13_topic-controlled-baseline-topics.log`
- `slop/verification/2026-09-13_topic-controlled-unique-topics.log`
- `slop/verification/2026-09-13_topic-controlled-aligned-comparison.log`
- `slop/verification/2026-09-13_topic-controlled-public-examples.log`

## Observations

- Frozen event IDs were identical: 12,179 in both runs.
- The frozen assigned corpus contained 11,059 unique `(author, content)` pairs, or 1,120 extra repeated rows (9.20%). Because the variant patch was not preserved, this does not prove which cohort its fitting code used.
- After maximum-overlap alignment, 3,059 of 12,179 events changed cluster (25.12%).
- Baseline had six non-`mixed` labels; treatment had five. Thus the treatment did not reduce the aggregate `mixed` count.
- The trading cluster label changed from `trading · bots · sylunara` to `trading · bots · live`, removing one bot-specific token. Its event count increased from 323 to 809 and only 312 baseline events remained in its aligned treatment cluster (Jaccard 0.380).
- `bitcoin · price · sats` was stable: 552 overlapping events and Jaccard 0.768.
- Some clusters were unstable despite the small reweighting. Baseline `agents · register` aligned weakly with treatment `nlogpost · shots · build` (100 overlapping events, Jaccard 0.132). One baseline mixed cluster aligned to `news · photo · flickr` with only 15 overlapping events (Jaccard 0.012).

## Interpretation

The output is a materially different clustering: 25.12% of events moved after maximum-overlap alignment, it produced one fewer descriptive label, and several aligned overlaps were low. However, missing variant provenance and the earlier cohort mismatch prevent attributing these differences to a measured reweighting magnitude. The largest mixed clusters were not mainly exact `(author, content)` duplicates, consistent with the reviewer diagnosis.

Decision: do not use or promote this alternate clustering. Keep committed production clustering and live clusters unchanged. The subsequent fixed-centroid, fixed-membership label-only experiment is the reproducible causal test; it is documented separately.

— Pi/gpt-5.6-sol
