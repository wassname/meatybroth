# Titan nearest-centroid representatives and density-clustering brief

## Frozen selective snapshot

I opened one WAL-consistent read transaction on the canonical SDK database and exported only the Titan cohort, stored centroids/topic IDs, the root’s available one/two-hop edges, and moderation-list metadata. The source transaction lasted 12 seconds (2026-09-13 21:18:44–21:18:56 UTC). The resulting no-writer artifact is `titan-selective-snapshot.sqlite`, SHA-256 `2da2730a3a312aef98c319be1687b53702d1b24d4a3364d0fc044b30910ccf65`; `PRAGMA integrity_check` returned `ok`.

- Exact space: `64c3c581f1da5a1973fb22e9bae7d91988c2298f14b92cbc3f89f1019703bbf5`
- Backend/model: Bedrock `amazon.titan-embed-text-v2:0`, provider-managed revision/hashes
- Shape: 512 dimensions, normalized; all 6,475 post vectors and 16 centroids are finite 512-float blobs with norms within 0.000001 of one
- Coverage at snapshot: 13,871 eligible posts; 6,475 eligible/stored Titan vectors (46.7%); 6,251 current topic memberships; 224 vectors arrived after the last cluster build and were unassigned
- Ledger: 7,151 requests, 2,774,145 returned tokens, $0.05548290 recorded, one uncertain request. Request count is not post count because long posts use chunks and attempts are durable.
- Social data: 109 root direct follows, 34 with a stored contact list, 13,358 observed two-hop authors. Missing lists still make “no observed path” weaker than nonconnection.
- Moderation: 143 unique canonical NSFW members and 13 spam members; neither set overlaps authors in the assigned Titan cohort

This is still a biased partial cohort. Embedding work selects pending posts by binary event ID. Exported IDs end at `6cc787…`; 1,006/6,475 (15.5%) start with four zero hex digits, 353 with five, and 57 with six. The current topics therefore overweight low-ID/NIP-13 proof-of-work posts and omit the remaining 53.3% of eligible posts. They must not be presented as full-corpus topics.

The rejected earlier full-database backup attempt remains recorded in `titan-copy-attempt.txt`; it did not produce this artifact. The successful export used a bounded selective transaction and did not copy WAL files or interrupt pueue job 1401.

Supervisor spot-check: the snapshot hash, integrity, 6,475 vectors, 224 unassigned vectors and 6,251 memberships match. “Duplicate rows” counts all rows in repeated author/text groups, not only extra copies: cluster 2 has 154 such rows and 121 extra copies. Corrected the space ID against snapshot metadata. — Pi/OpenAI

## Current algorithm

`src/embed.rs::cluster_topics` reads eligible vectors for one exact embedding space and 30-day window, ordered by event ID. It chooses

$$k = \operatorname{clamp}_{6,16}\!\left(\operatorname{round}\sqrt{n/25}\right),$$

uses the first vector followed by greedily farthest vectors as initial centroids, assigns by maximum cosine similarity, normalizes each mean centroid, and repeats for eight rounds. At 5,222 vectors the heuristic selects 14 clusters. Every post is assigned; there is no noise result. Keyword labels then require each displayed word in at least 20% of cluster posts and return Mixed if fewer than two words qualify.

Nearest-centroid posts are the right first naming evidence for this implementation: they show what each fitted mean most resembles. They are not sufficient by themselves. A plain label is warranted only when several nearest representatives and broader keyword support agree; otherwise the honest label remains Mixed. Centroid-nearest posts can also be generic averages rather than recognizable subjects.

## Actual representative review

I decoded the frozen vectors and ranked each assigned post against its stored centroid. `titan-centroid-nearest-details.md` records the top three IDs and short verbatim quotes for all 16 clusters; `titan-centroid-nearest.json` retains the top five with full content and evidence fields.

Proposed current-snapshot labels:

- Cluster 0: `gm · greetings`
- Cluster 1: retain `Mixed`; representatives are a timestamp-like string, Chinese prose, a Portuguese site link, “Test”, and an image URL
- Cluster 2: `trading · signals · sylunara`
- Cluster 3: `diversz · agent bootstrap`
- Cluster 4: `syndicated news · photos`
- Cluster 5: `casino · reviews`
- Cluster 6: `bitcoin · media · audits`
- Cluster 7: `nostr · event references`
- Cluster 8: `takaichi · japanese politics`
- Cluster 9: `trump · iran · strikes`
- Cluster 10: `bitcoin · price · sats`
- Cluster 11: `70u · drug ads`
- Cluster 12: `hashtag · search links`
- Cluster 13: `block · mempool · bitcoinfees`
- Cluster 14: `video · mp4`; this is a content-format group, not a semantic subject
- Cluster 15: `aum · telegram recruitment`

The clearest centers are often automation or campaigns, not ordinary conversation. For example:

- Cluster 2’s nearest posts are near-identical “BTC SHORT [...] conviction map [...] trading lanes” promotions; 154/218 rows are same-author/text duplicates and 217/218 have no stored social path.
- Cluster 3’s five nearest posts are the same DiversZ agent-bootstrap advert repeated 62 times by one author.
- Cluster 11’s nearest posts are the same Japanese incapacitating/date-rape-drug advert across seven authors.
- Cluster 12 consists near its center of unrelated hashtags plus search/link-farm URLs.
- Cluster 15’s identical Aum/Telegram recruitment text appears across ten authors.

The benign controls matter: clusters 10 and 13 are coherent Bitcoin price and mempool-fee bots, while cluster 0 contains many socially observed GM greetings. Automation, duplicate density, or no stored path is not sufficient evidence for suppression.

Canonical moderation lists contribute no positive classification evidence here: zero assigned authors overlap their 143 NSFW or 13 spam members. Social evidence is consistent with several promotion clusters but incomplete. No cluster was hidden or deleted from this review.

## DBSCAN and HDBSCAN

### DBSCAN

Scikit-learn’s clustering guide describes DBSCAN as viewing “clusters as areas of high density separated by areas of low density” and says clusters can have arbitrary shape. It does not require a cluster count and can leave points as noise. However, it requires global `eps` and `min_samples`. The guide says `eps` is “crucial to choose appropriately”: too small makes most points noise; too large merges clusters and can eventually merge the dataset. [Scikit-learn clustering guide, DBSCAN](https://scikit-learn.org/stable/modules/clustering.html#dbscan)

That global radius is a poor first assumption here. Duplicate promotions create dense pockets, short contextless replies can create other pockets, and ordinary topics plausibly have different densities. On normalized vectors, Euclidean distance and cosine similarity are monotone-equivalent because

$$\lVert x-y\rVert_2^2 = 2(1-x^\top y),$$

so a Euclidean implementation can preserve cosine neighborhoods. But high-dimensional tree indexes often lose their advantage. The same guide warns that implementations may construct a full pairwise matrix when trees cannot be used, consuming $n^2$ floats, and suggests sparse radius graphs or duplicate compression. At roughly 14,000 posts, a dense float32 distance matrix alone is about 0.73 GiB before algorithm state. [Scikit-learn clustering guide, DBSCAN memory notes](https://scikit-learn.org/stable/modules/clustering.html#dbscan)

### HDBSCAN

The same guide says DBSCAN assumes density is “globally homogeneous” and may struggle with different densities, while HDBSCAN “explores all possible density scales.” It constructs a mutual-reachability hierarchy and can obtain DBSCAN-like partitions across all epsilon values. It therefore removes a single global epsilon, not all choices: `min_samples`, `min_cluster_size`, metric and cluster-selection rule still affect the result. [Scikit-learn clustering guide, HDBSCAN](https://scikit-learn.org/stable/modules/clustering.html#hdbscan)

HDBSCAN may be a useful future alternative because it can choose a variable number of clusters, mark weakly supported posts as noise and represent multiple density scales. It is not a substitute for the user’s current explicit DBSCAN deliverable. Neither method solves the observed input problems: repeated posts alter local density, and `🤝` still lacks its stored parent’s AI-regulation context. Density clustering could correctly call much of the corpus noise without producing reader-useful named groups.

## Current requested deliverable

The user explicitly requested both DBSCAN and fixed-$k$ k-means in the deployed reader. That supersedes my earlier recommendation to use HDBSCAN as the first density comparison. Preserve the current k-means result and add DBSCAN as a separately named browse mode; do not silently replace one with the other. DBSCAN noise must remain a visible outcome rather than being forced into a topic.

The k-means representative review above supplies the pre-deployment baseline. For DBSCAN, inspect representative core posts, boundary posts and noise. Report the chosen `eps`, `min_samples`, distance definition and coverage because DBSCAN removes fixed $k$ but not those decisions. Judge both by quoted representatives, cluster coverage and noise composition—not by number of clusters, silhouette alone or attractive labels. HDBSCAN remains only a possible later alternative if one global DBSCAN radius proves inadequate.

No model, API, AWS or production call was made for this brief.

-- Pi/gpt-5.6-sol
