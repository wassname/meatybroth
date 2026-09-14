# K=24 contrastive topic labels

## Method

The default fixed-k count changes from 12 to 24. `MEATYBROTH_KMEANS_K` remains the actual rebuild parameter when set. DBSCAN epsilon, minimum samples, core/noise decisions, assignment, and `Unsorted` behavior are unchanged.

Labels use every member document in the selected-space cluster. URLs beginning with `http://`, `https://`, or `www.` are removed. Tokens retain Unicode letters/numbers plus internal `_`/`-`, must contain a letter, and are limited to 40 characters. The pinned English stop list applies only to ASCII matches; non-ASCII tokens are not discarded as presumed stopwords.

For term $t$ in cluster $c$:

$$s(t,c) = \frac{df_c(t)}{|c|}\log\left(\frac{N}{df(t)}\right)$$

where $df_c(t)$ is the number of cluster-member documents containing the term, $df(t)$ is its selected-space corpus document frequency, $|c|$ is cluster size, and $N$ is corpus size. A displayed term must:

- occur in at least $\max(2,\lceil |c|/10\rceil)$ member documents;
- have cluster document rate at least 1.5 times corpus document rate; and
- occur in at least two centroid-nearest representatives when two exist.

Up to three terms are shown in score order. With no qualifying term, the label is exactly `Unlabelled topic`. These checks are a deterministic labeling heuristic, not confidence or a claim that embedding coordinates are interpretable.

## Counts

Picker text is `label (count, percent%)`. Count is the number of current membership rows for the selected embedding space, grouping method, and topic. The percentage denominator is the sum of all those membership rows. For DBSCAN this includes `Unsorted`; the collapsed grouping details state that denominator. Percentages describe population share, not label confidence.

## Frozen review

The retained snapshot is `.local/integrated/p0-frozen.sqlite`. Its MiniLM space has 22,211 vectors, but its old topic tables assigned 22,210. The controlled after database removes that one unassigned vector before rebuild, so the before and after artifacts both have 22,210 assigned posts. The normal after rebuild, saved separately as `*-after-current-*.json`, has all 22,211 vectors.

| grouping | before topics | after topics | before population | after population |
|---|---:|---:|---:|---:|
| fixed-k | 12 | 24 | 22,210 | 22,210 |
| DBSCAN | 137 | 202 | 22,210 | 22,210 |

The fixed-k comparison controls membership exactly. The stored DBSCAN baseline has the same recorded epsilon (0.20) and minimum samples (8), but rebuilding the same retained vectors produces 202 rather than 137 groups. The source inspection did not find a DBSCAN-setting change in this work. I therefore do not attribute that group-count change to contrastive labels; the before/after DBSCAN artifacts are useful label evidence, not a label-only clustering comparison.

Positive fixed-k checks from the protocol anchors:

- Trump anchors `9c98…` and `0000…` are in `finally · trump · weak` (topic 18); all three displayed terms occur in at least two nearest representatives.
- The three trading-template anchors are together in `sylunara · bots · positions` (topic 9); each term occurs in all five representatives.
- The Bitcoin price anchors are together in `sats · price` (topic 21); both terms occur in all five representatives.
- Unicode-heavy French and Spanish template anchors are in fixed-k topic 8, now `Unlabelled topic`. This is preferable to its earlier weak `les · photo · flickr` label: each of those terms appeared in only one nearest representative.

A counterexample remains: the English trending-template anchor `385b…` is in `news · photo · scene` (topic 11). The label is supported by its five nearest representatives but reflects template vocabulary rather than a useful human topic. DBSCAN also splits the trading and Bitcoin templates across several tighter groups and leaves many protocol anchors as `Noise / unmatched`; that behavior is not claimed as an improvement here.

Artifacts:

- `slop/verification/2026-09-14_topic-labels-before-{kmeans,dbscan}.json`
- `slop/verification/2026-09-14_topic-labels-after-{kmeans,dbscan}.json` — controlled 22,210-post comparison
- `slop/verification/2026-09-14_topic-labels-after-current-{kmeans,dbscan}.json` — normal 22,211-vector rebuild
- `slop/verification/2026-09-14_topic-label-summary.log`
- `slop/verification/2026-09-14_topic-label-examples.log`
- `slop/verification/2026-09-14_topic-label-anchor-review.log`

-- Pi/gpt-5.6-terra
