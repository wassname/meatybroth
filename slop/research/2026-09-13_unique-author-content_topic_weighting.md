# Unique-author/content topic weighting experiment

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
- The corpus contained 11,612 unique `(author, content)` pairs, so this changed the fitting weight for 567 rows (4.66%).
- After maximum-overlap alignment, 3,059 of 12,179 events changed cluster (25.12%).
- Baseline had six non-`mixed` labels; treatment had five. Thus the treatment did not reduce the aggregate `mixed` count.
- The trading cluster label changed from `trading · bots · sylunara` to `trading · bots · live`, removing one bot-specific token. Its event count increased from 323 to 809 and only 312 baseline events remained in its aligned treatment cluster (Jaccard 0.380).
- `bitcoin · price · sats` was stable: 552 overlapping events and Jaccard 0.768.
- Some clusters were unstable despite the small reweighting. Baseline `agents · register` aligned weakly with treatment `nlogpost · shots · build` (100 overlapping events, Jaccard 0.132). One baseline mixed cluster aligned to `news · photo · flickr` with only 15 overlapping events (Jaccard 0.012).

## Interpretation

The proposed weighting plausibly fixes the narrow duplicate-vote mechanism in the trading label, but it is not a reliable global improvement. A 4.66% fitting-weight change moved 25.12% of event memberships, produced one fewer descriptive label, and substantially changed several clusters. The largest mixed clusters were not mainly exact `(author, content)` duplicates, consistent with the reviewer diagnosis.

Decision: reject this change in its present form. Keep committed production clustering and live clusters unchanged. A later experiment would need a less seed-sensitive centroid procedure or deduplication limited to keyword support after fixed centroid fitting, with the same aligned-membership review.

— Pi/gpt-5.6-sol
