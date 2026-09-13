# Unique-author/content keyword-support experiment

## Decision

Reject this variant. Deduplicating `(author, content)` only for keyword document frequency preserved all 12,179 event assignments, but changed two labels and neither change was a clear improvement:

- `agents · register` → `mixed`
- `connecting · connection · service` → `connecting · connection · error`

The suspected `trading · bots · sylunara` duplicate-vote label did not change on this frozen current corpus.

## Controlled method

Both runs used identical database copies, 12,179 eligible event IDs, vectors, 16 centroids, assignments and production `topic_label`. The treatment changed only the text iterator passed to `topic_label`: one vote per unique `(author, content)` inside each fixed cluster. It did not change centroid fitting, filtering, assignment or public APIs.

The exact temporary source patch is `slop/research/2026-09-13_topic-label-only-unique.patch`. The run command and raw output are preserved in:

- `slop/verification/2026-09-13_topic-label-unique-recluster.log`
- `slop/verification/2026-09-13_topic-label-unique-comparison.log`
- `slop/verification/2026-09-13_topic-label-unique-fixed-examples.log`
- `slop/verification/2026-09-13_topic-label-unique-production-restore-build.log`

The comparison query initially reused a one-statement CTE in a second SQL statement and failed with `no such table: bs`; the corrected query explicitly qualified `main` and `baseline`. This reporting error did not alter either clustering run.

## Controls and observations

> `12179  12179  0`

All baseline and treatment assignments had the same topic ID. This is the direct control that was missing from the centroid-reweighting experiment.

The frozen assigned corpus contained 11,059 unique `(author, content)` pairs among 12,179 events. The treatment therefore removed 1,120 repeated votes (9.20%) from label support only.

The fixed examples show that baseline topic 5 (`agents · register`) is heterogeneous: autonomous-agent advertisements, JSON price cards, status strings, art prose and service messages. Losing its label does not expose a coherent replacement topic. Topic 7 retained `connecting · connection` and merely replaced `service` with `error`. Topic 13 retained the suspected trading label unchanged.

For the earlier centroid-reweighting variant, recomputing farthest-first initialization on the exact frozen rows found all 16 initial centroid event IDs identical between baseline and unique-weight candidates (`slop/verification/2026-09-13_topic-initial-centroid-identities.log`). Therefore the observed 25.12% aligned membership change came from different iterative centroid weights, not different initial seeds.

## ml-debug form

| row | answer |
|---|---|
| log length; config | Comparison: 20 nonblank lines after the corrected query. Baseline and treatment: 12,179 IDs, MiniLM space `bd46…644e`, 16 clusters; only keyword-support deduplication changed. |
| each `SHOULD:` line | None in this short unsupervised run. The pre-run discriminator was zero changed assignments; observed `changed_topic_id=0`. |
| cited-number null | The control is production clustering on the identical frozen corpus. Random topic labels are not a useful null for label readability. |
| init before update | N/A: cached vectors and fixed baseline centroids/assignments; no model training. |
| dummy/simple heuristic | Production raw document frequency is the baseline. It retained six non-`mixed` labels; treatment retained five. |
| baseline on val/held-out | No held-out corpus was run. This blocks a positive generalization claim, but not rejection on the frozen corpus because the treatment already failed its intended label case. |
| schedule | N/A; no optimizer or training schedule. |
| one full sample | `topic-label-unique-fixed-examples.log` includes fixed event IDs and raw text. Topic 5 includes `DiversZ Commons — CALL_FOR_AIS...` alongside an XMR JSON price card and unrelated prose. |
| worst step, loss/grad | N/A; no loss or gradients. The worst observable change was topic 5 becoming `mixed`. |
| surprising lines | `agents · register → mixed` was worse than expected; explained: removing repeated author/content support pushed all surviving terms below the label threshold in an already heterogeneous cluster. `trading · bots · sylunara` unchanged; explained: the current larger frozen corpus differs from the earlier 4,920-row diagnostic corpus. |
| missing evidence | Human blind preference on labels across another frozen time window; per-token raw versus unique author support for every candidate term. These would be needed to claim a general improvement. |
| diagnoses | 65%: exact duplicate keyword votes are not the main current label failure (trading unchanged; mixed count worsened). 20%: unique support removes useful repeated evidence in heterogeneous clusters (`agents` lost). 10%: corpus-time dependence explains disagreement with the earlier threshold diagnostic. 5% unknown. |
| fresh review | Reviewer wrote: “narrow label mechanism as real but global treatment rejected”; this label-only run directly tested the remaining narrow variant and found no trading-label change. |
| cheapest separator | Already run: fixed centroids/assignments with label-only dedup. It predicts zero membership churn and a changed trading label if duplicate DF was decisive. Observed zero churn but unchanged trading label. |
| wall-clock/resources | Build plus recluster plus restore: 17 seconds. GPU memory not applicable; CPU/RAM peak was not recorded. |

## Second cause and three ways the conclusion could be false

The result rests on unchanged trading text and two degraded label changes. Another cause is the discrete label threshold: deduplication might improve token quality while pushing all candidates below threshold. A per-token support table would separate quality from thresholding.

The rejection could be too broad if (1) another time window has much heavier exact duplication, (2) author-level rather than `(author, content)` support is the correct unit, or (3) a threshold calibrated to unique support recovers useful labels. This run rejects only the tested fixed-threshold `(author, content)` variant, not all duplicate-aware label methods.

Production source, binary and live clusters were restored and left unchanged.

— Pi/gpt-5.6-sol
