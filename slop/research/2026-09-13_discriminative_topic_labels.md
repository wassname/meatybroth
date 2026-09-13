# Discriminative topic-label experiment

## Decision

Reject global promotion. The variant reduced `mixed` labels from 10 to 1 with zero membership changes, but achieved that by labeling clusters from terms present in as little as 2–6% of their posts. Several labels therefore imply coherence that the fixed examples do not show.

## Method

An isolated worktree at `1d36733` ran production clustering on a database copy. Centroids, 12,179 event assignments, filters and cluster count were unchanged. Only labels changed: candidate terms required at least 2% within-cluster event support, five events and three distinct authors, then ranked by within-cluster support times inverse global document frequency. The exact variant is `slop/research/2026-09-13_topic-discriminative-labels.patch`.

Raw evidence:

- `slop/verification/2026-09-13_topic-discriminative-comparison.log`
- `slop/verification/2026-09-13_topic-discriminative-fixed-samples.log`
- `slop/verification/2026-09-13_topic-discriminative-label-scout.log`
- `slop/verification/2026-09-13_topic-discriminative-recluster.log`

## Observations

> `12179  12179  0`

All event assignments remained fixed.

Clear or plausible changes included:

- cluster 1: `mixed` → `bitcoin · crypto · money`; event support 28.5%, 14.2%, 11.8% across 158, 55 and 62 authors;
- cluster 13: `trading · bots · sylunara` → `trading · trade · signals`; support 90.7%, 77.7%, 55.7%, but only 3–6 authors, accurately describing a concentrated campaign;
- cluster 9: `bitcoin · price · sats` → `bitcoin · price · block`.

Misleading or weak changes included:

- cluster 0: `morning · day`; support only 5.1% and 2.5%. Its newest fixed examples discuss a treadmill, a baby and financial support for Quebec;
- cluster 3: `nostr · 70u · news`; secondary terms have about 2.4–3.1% support;
- cluster 11: `yas · weekend · marina`; selected-term support is 14–16%, while newest examples discuss flights, cooking and driving from Dubai;
- cluster 12: `agent · tech · data`; support is 5.9–7.6%, and fixed examples include SEO hashtag spam and AI promotion.

Cluster 6 remained `mixed`, mostly because its Japanese text produced too few ASCII candidates.

## Interpretation

Global inverse document frequency finds discriminative tokens, but distinctiveness is not cluster coherence. The 2% floor permits a label that 95–98% of posts omit. This makes the UI more specific without making the underlying broad clusters more coherent.

The two clearest changes suggest a later selective rule might label only clusters with materially higher term coverage and author diversity. That is not promoted here: a new threshold without validation would move the same problem rather than solve it.

Production source, binary, live database and live clusters were untouched. The isolated worktree remains disposable.

— Pi/gpt-5.6-sol
