# Frozen topic-label review protocol

Read-only review of the current label path in `62a48237` and the frozen controlled MiniLM database `topic-baseline-controlled.sqlite`. No label, cluster, filter, or application change was made.

## Current path and its limit

`src/embed.rs:1167-1207` labels each K-means/DBSCAN cluster from its five nearest-centroid texts only. It removes URLs, accepts only ASCII tokens, removes English stopwords, deduplicates a token within each text, and requires support in `max(2, ceil(3 × 5 / 5)) = 3` representatives. Fewer than two terms becomes `mixed`.

This is a deterministic heuristic, but it is not a label from all members. It will miss a cluster’s dominant non-English terms and can give a nearby repeated campaign or news-template vocabulary disproportionate weight.

## Fixed before samples

These are review anchors, not labels to preserve after K=24/DBSCAN changes. They are current K=16 MiniLM member IDs from the frozen database, selected to expose distinct failures.

| case | current topic / label | member IDs to inspect | observation |
|:---|:---|:---|:---|
| Trump/news miss | 2 / `trump · news` | `9c98c9e825375fcb…`, `0000f5e7fdec5153…`, `e0969ce1a2863cd3…` | The first two discuss Trump/Iran; the third is a generic multilingual trending-hashtag list. A Trump label does not describe all shown members. |
| Trading campaign | 13 / `trading · bots · sylunara` | `b00b76187022727b…`, `c8153a5d15a089b…`, `f9d232c425c53d69…` | Repeated single-author trading-call promotion with URL/template variation. This is a campaign pattern, not evidence that all trading is unwanted. |
| non-English / script observation | 15 / `news · photo · flickr` | `cf66c9f2f42f6dc…`, `1d5d4c09ded87d8…`, `6d707d26238b89ac…` | French, French/Senegal, and Spanish text occur. This is an observation of script/language in samples, not language identification for the cluster. English-only label terms discard the visible topical language. |
| coherent legitimate cluster | 9 / `bitcoin · price · sats` | `251ebecf6e2ca3c0…`, `df50ece794ec92f3…`, `7928dbd74fe561d…` | Bitcoin price/block/mempool updates are coherent and the current label is understandable. This is the positive control. |
| template/news failure | 2 / `trump · news` | `e0969ce1a2863cd3…`, `808772c784c5eebe…`, `385bf23a8d2d064b…` | Three generic “trending topics” templates enter the Trump-labelled cluster. This is the negative control for a label that is locally true but cluster-misleading. |

## K=24 / DBSCAN before→after judgment

Use one frozen retained event/vector snapshot, selected embedding space, 30-day cutoff, and admission state for both algorithms. Do not compare a K=16 membership list to a later rolling corpus as though label changes caused the movement.

For each K=24 topic and each DBSCAN non-noise topic:

1. Save its member event-ID set, count, five nearest-centroid IDs and cosine scores, all candidate label terms, and the exact space/configuration (`k=24`; DBSCAN epsilon/min-samples). Save DBSCAN noise separately.
2. Form candidate terms from **all member texts** after URL removal, using Unicode letter/number tokenization. Do not discard a token merely because it is not ASCII. Use one normalized token form consistently for matching; record the raw displayed form beside it.
3. For every proposed displayed term, record: number and fraction of member events containing it; number and fraction of events in other current topics containing it; and which of the five nearest-centroid posts contain it. A term absent from every nearest-centroid post is a review warning, not a hidden substitution.
4. Judge labels contrastively: a term needs both member support and lower outside-topic support. Prefer a plain `mixed`/`unlabelled` label when no two terms make the cluster distinguishable. Do not use an embedding projection, activation lens, UMAP coordinate, or language-ID claim as a reader label without separate evidence.
5. Manually inspect the five fixed anchor sets above after reassignment. Mark each as: coherent label; changed membership explains difference; campaign/template dominance; non-English label failure; or no useful label. Save misses as well as attractive labels.
6. For DBSCAN, show noise as `Unsorted` with its count; do not equate noise with spam. A label or campaign observation is not itself a filter action.

A proposed label is ready for reader copy only when its terms are present in the inspected nearest-centroid posts and its all-member/outside-topic counts make the label more specific than a generic term. This is a judgment protocol, not a numerical quality guarantee.

## Later filter review boundary

When the app owner supplies the explicit filter, review only its stated boundary: warning label versus body, matching only `auto-spam`, URL and paging preservation, raw-event retention, and no change to warnings that are merely visible. This label review does not authorize filtering a topic, author, long post, URL-heavy post, non-English post, or `GM` post.

-- Pi/gpt-5.6-terra
