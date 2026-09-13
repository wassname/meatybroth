# Practical feed engineering around text embeddings

Research date: 2026-09-13  
Resolved model: `gpt-5.6-sol` (Intercom roster); provider was not independently exposed.  
Scope: source review only. No application, database, API, model or deployment change was made.

## Answer first

There is no well-supported universal rule such as “exclude one-word posts.” The strongest code evidence is narrower:

- Older Twitter code has a URL-stripped word/character threshold for *out-of-network push recommendations*. The threshold varies with language and whether the post has media or a quote. One branch uses five words. The filter is disabled and its configurable thresholds are zero in the public defaults, so the snapshot does not reveal production settings.
- The inspected current X Home Mixer filter pipeline has no word-count or text-length filter. It filters exact IDs, out-of-network replies/retweets, old/seen/served posts, social and safety conditions, then ranks and applies conversation deduplication. This says nothing about unshipped upstream corpus processing.
- Nalgorithm excludes replies because it does not supply their parent context, but combines quote text with the quoted post and resolves boosts to the original. Current X similarly rejects out-of-network replies while allowing in-network replies with ancestors through this particular filter.
- Mastodon excludes replies from *trending eligibility* without deleting or hiding them elsewhere. Maintained Bluesky templates also make reply exclusion a feed-specific option.

For Meatybroth, keep short posts in storage and Latest/Conversations. Remove only contextless, low-information forms from default topic fitting and semantic candidate retrieval. Keep a separately browsable short/greetings class. If a reply's parent is present, use the thread relation instead of pretending the reply text is standalone. This addresses topic pollution while preserving “GM,” emoji reactions and useful bots.

The first engineering change worth trying is exact, versioned embedding-input deduplication for clustering and feed slates, followed by an author cap or decay. The frozen Titan review already shows that duplicate campaigns dominate several centers, while coherent GM, price and fee bots are benign counterexamples. A minimum-word threshold would miss the main failure and damage those controls.

## Keep four decisions separate

A post can be valid in one surface and unsuitable in another.

| Decision | Recommended treatment of one-word, emoji-only, URL-only and short replies |
|---|---|
| Canonical storage | Keep admitted posts. Moderation/deletion policy remains the authority. |
| Embedding | Continue embedding ordinary short standalone text for now. For future unpaid work, skip or reuse vectors for empty-after-normalization URL/media shells. Do not mix parent-augmented and standalone vectors in one undocumented space. |
| Topic clustering / semantic candidates | Default-exclude URL-only/media-only shells, pure reaction replies and replies whose parent is unavailable. Put coherent short standalone posts in a visible `Short / greetings` group rather than treating them as spam. |
| Reader display | Keep them in Latest and Conversations. A feed-specific omission is not global hiding. Show an explanation such as “not in semantic topics: reply context unavailable.” |

This classification should be metadata, not deletion. It can be changed without re-collecting events.

### Why parent context changes the answer

A token such as `🤝`, `yes`, or an image URL can be a meaningful reply and a useless standalone embedding. Three source paths encode that distinction:

1. [Current X filters out out-of-network replies and replies with no ancestors](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/home-mixer/filters/oon_retweet_reply_filter.rs#L13-L18), but this filter permits in-network replies with ancestors.
2. [Nalgorithm filters replies](https://github.com/jooray/nalgorithm/blob/bbacb1e0161e8bd690c38c65a31df90cd5e15d5b/lib/src/fetcher.ts#L248-L251), while [its README says quote text and the embedded post are scored together](https://github.com/jooray/nalgorithm/blob/bbacb1e0161e8bd690c38c65a31df90cd5e15d5b/README.md#L15-L22).
3. [Mastodon excludes replies only from trending eligibility](https://github.com/mastodon/mastodon/blob/a7cc70db674d79616597fcc1e67aafee6fcb2f11/app/models/trends/statuses.rb#L91-L98).

The transferable rule is “judge the available semantic unit,” not “short replies are bad.” For Meatybroth's existing vector space, assigning a short reply to its embedded parent's topic is cheaper and clearer than making a new parent-plus-reply Titan input. If parent-augmented embeddings are later introduced, give them a distinct input-version/space identifier and re-embed consistently.

## What X/Phoenix actually contributes

Phoenix is a learned personalized recommender, not clustering over generic text vectors.

> Phoenix is a recommendation system that predicts user engagement (likes, reposts, replies, etc.) for content. It operates in two stages:
>
> 1. **Retrieval**: Efficiently narrow down millions of candidates to hundreds, scoring a user embedding against a precomputed candidate index
> 2. **Ranking**: Score and order the retrieved candidates using a more expressive transformer model

Source: [xAI's Phoenix README](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/phoenix/README.md#L38-L45), an author description of its own released code.

Its candidate vectors include learned semantic IDs from multimodal embeddings and hashed authors. Retrieval uses a user tower over engagement history. Titan text embeddings have neither that training objective nor that user representation. The parts that transfer without a personalized model are the pipeline boundaries:

1. generate a larger candidate pool from several sources;
2. filter eligibility and previously seen content;
3. score quality/relevance/freshness;
4. rerank the slate for repetition and conversation diversity;
5. retain served history.

Current X uses multiple sources (Thunder, Phoenix, SimClusters, Tweet Mixer and cache) in the [candidate pipeline](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/home-mixer/candidate_pipeline/phoenix_candidate_pipeline.rs#L320-L330). Its [filter list](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/home-mixer/candidate_pipeline/phoenix_candidate_pipeline.rs#L354-L381) handles eligibility before scoring. Exact duplicate IDs are removed, while post-selection keeps only the best candidate per conversation.

X also discounts repeated authors after scoring:

> `fn diversity_multiplier(decay_factor: f64, floor: f64, exponent: f64) -> f64 {`
> `    (1.0 - floor) * decay_factor.powf(exponent) + floor`
> `}`

Source: [current X ranking scorer](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/home-mixer/scorers/ranking_scorer.rs#L625-L672). Candidates are first sorted by their pre-diversity score, then the first, second and later posts from an author get different multipliers.

This is more useful here than copying Phoenix's model. Meatybroth can retrieve by cluster or cosine similarity, rank by recency/similarity, then apply an author decay or hard cap. Do not copy X's multiplicative formula onto raw cosine scores: multiplying a negative cosine by a factor below one makes it less negative and therefore ranks it higher. Use a nonnegative calibrated base score, or use author interleaving/hard caps that do not assume a score scale.

### Short-text evidence from older Twitter code

The closest direct answer to the user's question is the old push-service path:

> `val afirfOonCandidatesWithoutMediaTweetWordLengthThreshold = 5`

Source: [Twitter out-of-network push quality predicate](https://github.com/twitter/the-algorithm/blob/c54bec0d4e029fe34926ef3258a86ccacc0d0182/pushservice/src/main/scala/com/twitter/frigate/pushservice/predicate/OutOfNetworkCandidatesQualityPredicates.scala#L51-L77).

That predicate removes URLs before counting words, varies the threshold based on media/quote and redacted language groups, checks a health/quality eligibility function, and runs only when `enableFilter` is true ([full decision path](https://github.com/twitter/the-algorithm/blob/c54bec0d4e029fe34926ef3258a86ccacc0d0182/pushservice/src/main/scala/com/twitter/frigate/pushservice/predicate/OutOfNetworkCandidatesQualityPredicates.scala#L78-L128)). It is unsuitable as evidence for a global five-word rule. It is evidence for conditional thresholds at candidate retrieval time.

In the public configuration, `EnablePrerankingTweetLengthPredicate` defaults false and configurable thresholds default to zero ([parameters](https://github.com/twitter/the-algorithm/blob/c54bec0d4e029fe34926ef3258a86ccacc0d0182/pushservice/src/main/scala/com/twitter/frigate/pushservice/params/PushFeatureSwitchParams.scala#L3724-L3801)). The actual production settings are not available.

## Deduplication is the main transferable technique

There are several distinct duplicate problems:

| Duplicate type | Evidence and suitable action |
|---|---|
| Same event from several relays/sources | Deduplicate by event ID before work. Current X does this by tweet ID. Nalgorithm does it after multi-relay fetch. |
| Repost/boost of the same event | Resolve to original ID for scoring; retain booster social proof. Nalgorithm reports 75 fewer scoring keys in a real 500-post sample by doing this. |
| Same conversation | Keep the best post or group the thread. Current X's post-selection filter keeps the best candidate per conversation. |
| Exact repeated embedding input | Hash the versioned Titan input; use one prototype for topic fitting and one occurrence per feed page. Preserve occurrence count as a campaign feature. |
| Near duplicate/template with changing numbers/URLs | Only collapse with corroboration: high vector similarity plus shared normalized text/author/template. Pure cosine is unsafe for price and fee bots. |
| Previously shown item/family | Keep a small served-history table. Current X uses IDs and Bloom filters; older Twitter code also filters previously viewed semantic cluster IDs. |

Older Twitter Home Mixer is unusually direct about semantic-family deduplication. It removes candidates sharing `0.95` or optional `0.88` cluster IDs with earlier candidates, and can remove a cluster seen within a historical window ([source](https://github.com/twitter/the-algorithm/blob/c54bec0d4e029fe34926ef3258a86ccacc0d0182/home-mixer/server/src/main/scala/com/twitter/home_mixer/functional_component/filter/ClusterBasedDedupFilter.scala#L76-L121)). The file does not reveal how those IDs were built. Copy the idea, not the thresholds.

For the observed Titan corpus, first hash the exact versioned text sent to Titan. This gives an auditable identity rule: posts with the same hash had the same embedding input. Only test broader normalization as a separate, versioned near-duplicate rule after inspecting collisions. Lowercasing can merge meaningful `US`/`us` text, and URL/query changes can identify different resources or measurements. Replacing numbers or removing URLs can also merge price, fee, release and incident bots. Preserve numbers, domains, hashtags, case and emoji in the first exact rule.

### How to use duplicates in clustering

Do not let 62 identical Titan inputs move a centroid 62 times. Fit k-means on one prototype per exact input hash, then assign every original post to the resulting center. Report both prototype count and raw post count.

For DBSCAN, duplicate multiplicity is part of density. Deduplicating the fit can change which points satisfy `min_samples`, so the choice is not neutral. Keep two views:

- reader-topic DBSCAN over unique prototypes, aimed at subject discovery;
- campaign diagnostic showing raw duplicate count per resulting cluster.

Report raw-post and prototype populations beside `eps` and `min_samples`. Keep benign identical GM posts visible even if the prototype becomes noise after deduplication. This still uses DBSCAN as requested and avoids silently declaring “spam” from density. If sample weights or raw multiplicity are later used, report that choice because it materially changes core points.

## Spam, automation and density are different labels

The frozen Titan review gives concrete counterevidence to simple suppression:

- cluster 2 has 154/218 same-author/text duplicates and looks like a trading promotion;
- cluster 3 repeats the same DiversZ advert 62 times;
- clusters 11 and 15 repeat drug/recruitment campaigns across authors;
- cluster 0 is coherent socially observed GM greetings;
- clusters 10 and 13 are coherent Bitcoin price and fee bots.

The current moderation lists overlap none of the assigned Titan authors. Social paths are incomplete. Therefore:

- `duplicate_family_size`, `unique_author_count`, `moderation_label`, `social_path`, and `automation_pattern` should remain separate fields;
- DBSCAN noise means “not in a dense region under these parameters,” not “spam”;
- dense means repeated or common, not bad;
- an author cap improves a page even when the author is benign;
- only a reviewed moderation rule should hide content globally.

X follows the same architectural separation: safety/visibility filters, social-graph filters, ID deduplication, conversation deduplication and author diversity are separate components in the pipeline. Its exact ID filter does not infer spam ([source](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/home-mixer/filters/drop_duplicates_filter.rs#L8-L25)).

## Candidate generation, ranking and slate composition

A simple non-personalized feed can still use the production pattern.

### Candidate sources

- *Topic*: members of the selected k-means cluster or DBSCAN cluster.
- *Similar*: top cosine neighbours in the exact selected embedding space.
- *Fresh*: recent eligible posts, including unassigned posts between rebuilds.
- *Conversation*: parents/replies connected to candidates.
- *Exploration*: a small recent sample outside the selected cluster, labelled as such.

Union candidates by event ID. Retain source reasons so the UI can say why an item appeared.

### Eligibility

Apply moderation, deleted/expired state and feed-specific semantic eligibility. Do not use the topic cluster itself as a safety label. Keep a visible noise/unassigned feed, as the current plan requires.

### Base ranking

Use one understandable principle per feed:

- Topic: newest within topic, or centroid similarity with a monotonic freshness tie-break.
- Similar: cosine similarity, with age only as a bounded tie-break or explicit filter.
- Latest: reverse chronological.

Avoid a weighted sum of similarity, popularity, social path and freshness until there is evidence for how to balance them. The Princeton prototype shows the common architecture, but its hard-coded `0.5/0.3/0.2` weights are design choices, not evidence for Meatybroth ([source](https://github.com/Princeton-HCI/cos-atproto-pds/blob/9aad1247e52349f27f3b36468d07d411fccad16a/bluesky-feed-manager/server/algos/feed.py#L273-L331)).

### Slate composition

After base ranking:

1. collapse event/repost/exact-text duplicates;
2. group or cap one conversation;
3. interleave authors or apply author decay;
4. reserve freshness/exploration positions only if the feed otherwise becomes stale;
5. log the reason each candidate moved or disappeared.

AlgoRelay offers a small Nostr example: it groups candidates by author, distributes one post per author into each of five cached feed variants, adds viral candidates without duplicate authors, sorts each variant by score and rotates variants between requests ([source](https://github.com/barrydeen/algo-relay/blob/0c49f8597dc78534bfab8c17ac68b87306ccccc9/algorithm.go#L140-L224)). It is not embedding-based, but the author mixing and cached alternatives transfer directly.

## Open-source examples and what each really proves

### 1. X Phoenix and Home Mixer

- Code: [xai-org/x-algorithm](https://github.com/xai-org/x-algorithm/tree/6bb4594253cdfa9ea19983a54a401d5ce8f8275d)
- Status: current, large-adoption Apache-2.0 release; xAI calls the model/trainer/serving code production implementation.
- Useful parts: two-stage retrieval/ranking, explicit filters, seen history, conversation dedup, author decay, multi-source candidate union.
- Limitation: no production corpus/checkpoint/data orchestration. [The quickstart says synthetic execution is not quality evidence](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/phoenix/QUICKSTART.md#L3-L7). Semantic IDs from learned multimodal embeddings are not Titan text clusters.

### 2. Older Twitter algorithm

- Code: [twitter/the-algorithm](https://github.com/twitter/the-algorithm/tree/c54bec0d4e029fe34926ef3258a86ccacc0d0182)
- Status: maintained snapshot, AGPL-3.0, high adoption.
- Useful parts: conditional URL-stripped length filtering for a narrow push surface, semantic-family historical dedup, exponential author discount, and explicit candidate/filter/rank phases.
- Limitation: many feature-switch values and language groups are redacted or inert defaults. It should not be read as a complete current production configuration.

### 3. Nalgorithm

- Code: [jooray/nalgorithm](https://github.com/jooray/nalgorithm/tree/bbacb1e0161e8bd690c38c65a31df90cd5e15d5b)
- Demo: [cypherpunk.today/nalgorithm](https://cypherpunk.today/nalgorithm/) returned HTTP 200 at review time.
- Status: active and very recent, three stars, no detected license. Treat as a concrete demo rather than established practice.
- Useful parts: exact relay-event dedup, replies omitted when context is unavailable, quote plus quoted-content scoring, boosts keyed to their original, persistent incremental score cache.
- Limitation: LLM scoring costs calls and is personalized. It is not embedding retrieval and does not solve topic clustering.

### 4. AlgoRelay

- Code: [barrydeen/algo-relay](https://github.com/barrydeen/algo-relay/tree/0c49f8597dc78534bfab8c17ac68b87306ccccc9)
- Status: MIT, 54 stars, last inspected commit February 2025.
- Useful parts: Nostr-native score from author interactions, global engagement and exponential recency; one-author-per-variant distribution; cached alternative slates.
- Limitation: no embeddings, little evaluation, and the implementation's author distribution order depends on map/input order. Use it as readable code, not a validated ranking policy.

### 5. Bluesky feed-generator ecosystem

- Official starter: [bluesky-social/feed-generator](https://github.com/bluesky-social/feed-generator/tree/9224e985f7fa75b6f3ebfb83c98db514a28fb875), MIT, 2,064 stars. It defines the firehose → local index → feed-skeleton contract and recommends short retention for most feeds. Its included feed is keyword-based, not semantic.
- Maintained Python template: [MarshalX/bluesky-feed-generator](https://github.com/MarshalX/bluesky-feed-generator/tree/be500ba5be2c2006f0649c8ce8862943ac7966c3), MIT, 304 stars. It optionally filters replies and delayed archive imports before indexing. Its own comment warns delayed posts can also result from outages, a good example of a seemingly simple freshness rule losing valid data.
- Semantic research prototype: [Princeton-HCI/cos-atproto-pds](https://github.com/Princeton-HCI/cos-atproto-pds/tree/9aad1247e52349f27f3b36468d07d411fccad16a), MIT, zero stars. It uses MiniLM/pgvector to retrieve text and vector candidates, deduplicates by URI, enforces `MAX_PER_AUTHOR = 10`, filters to 48 hours and applies a composite score. Source inspection found that final “relevance” is keyword/profile matching, while embeddings generate candidates. This is exactly why “uses embeddings” does not imply a complete embedding feed algorithm.
- Local LLM example: [drbh/bsky-feeder](https://github.com/drbh/bsky-feeder/tree/7b51b85459b6f752f83d15ffa0cb92d98ea53f3d) classifies each firehose post with Qwen2-7B-Instruct and stores positive science posts. It is small, unlicensed and stale relative to the other examples; it demonstrates ingest-time classification, not robust ranking.

### 6. Mastodon trends

- Code: [Mastodon trending statuses](https://github.com/mastodon/mastodon/blob/a7cc70db674d79616597fcc1e67aafee6fcb2f11/app/models/trends/statuses.rb)
- Status: current production open source, AGPL-3.0, high adoption.
- Useful parts: surface-specific eligibility, language preference ordering, engagement threshold and time decay. Replies and sensitive posts are ineligible for trending, rather than globally removed.
- Limitation: this is a trend feed, not embeddings or personalized recommendations.

Searches found many semantic-news demos, but most were low-adoption wrappers around Sentence Transformers plus FAISS/pgvector. They added little beyond the Princeton code and often blurred retrieval with ranking. The shortlist above favors source paths with clear behavior over a longer link list.

## Ordered shortlist for Meatybroth

These are ordered by expected information gain and implementation cost. They do not replace the authorized k-means/DBSCAN deployment.

### 1. Collapse exact embedding-input duplicates in topic fitting and each feed page

- Change: hash the exact versioned Titan embedding input first. Fit topics on one prototype per hash. Assign all originals afterward. Show raw count and unique-content count. Keep at most one hash family per page. Evaluate any lowercasing or URL normalization later as a separate rule.
- Cost: likely tens of lines plus one indexed hash column. Hashing is linear in text size; database and memory cost are tens of bytes per post. No API spend and no extra vector calls.
- Could break: syndicated news with identical text from several useful sources; short generic strings such as `gm` all collide. Preserve author/source counts and make the full family browsable.
- Discriminator: the 62-copy DiversZ family and 154/218 BTC duplicate family move the fitted centers once each, while a GM topic remains accessible and counts are not lost.

### 2. Apply post-ranking author and conversation diversity

- Change: after base sorting, allow one post per author/conversation in the first pass, then fill remaining positions from deferred posts. A soft exponential decay is an alternative if hard interleaving leaves too few items.
- Cost: one map/set over a candidate page, approximately O(candidate count) time and memory. No API spend.
- Could break: a narrow feed with one authoritative live-blog or fee bot may become sparse. Fill from deferred candidates and state the cap.
- Discriminator: no campaign author occupies adjacent/top-heavy results; a single-author topic still fills after the first pass instead of returning an empty page.

### 3. Add semantic eligibility reasons, without global hiding

- Change: classify `ordinary`, `short-standalone`, `url/media-shell`, `reply-with-parent`, `reply-missing-parent`, and `reaction-only`. Use Unicode grapheme/URL/attachment/reply metadata, not an English word count. Default Topics/Meaning omit shells and missing-context replies; Latest/Conversations retain them.
- Cost: small deterministic ingest/query code and one enum/column. No model call. Existing embeddings can remain; the classification is reversible.
- Could break: a bare URL can be the useful post; language tokenization and emoji sequences make word counts unreliable. Provide the separate class and override/browse path.
- Discriminator: `🤝` with a parent remains in its thread; the same content without a parent is absent from default semantic candidates with an explicit reason; GM and fee/price bots remain visible.

### 4. Reuse thread context structurally before changing embeddings

- Change: for a short reply with an embedded parent, inherit the parent's topic for conversation navigation and show both in the result card. Do not silently store parent-plus-reply vectors under the current Titan space ID.
- Cost: one join at assignment/render time; no API spend. Batch joins to avoid one query per card.
- Could break: replies often change topic or disagree. Label inherited topic as parent-derived and keep the reply out of centroid fitting.
- Discriminator: manually inspect agreement and disagreement examples. A disagreement must still render and must not move the parent's centroid.

### 5. Add conservative near-duplicate diagnostics, then decide whether to suppress

- Change: within each cluster, report high-cosine pairs that also share an author/domain and substantial normalized character/token n-grams. Review before using it as a slate filter.
- Cost: brute force within large clusters can be quadratic. Restrict to current page/candidate pool or use existing centroid/nearest-neighbour results. No new embeddings.
- Could break: recurring price, fee, weather and release reports are intentionally templated. Do not erase numbers in the initial signature.
- Discriminator: promotion variants group, while Bitcoin price/fee controls with changed measurements remain distinct.

### 6. Add served-family history only after page-level repetition is fixed

- Change: record recent event IDs and duplicate-family hashes. Suppress or discount already served families for a bounded window.
- Cost: one small SQLite table and indexed lookups; a Bloom filter is unnecessary at this corpus size. No API spend.
- Could break: users may want to revisit sparse topics. Make it request/session scoped or expose “include seen.”
- Discriminator: refresh/pagination does not immediately repeat the same campaign, while direct links and explicit include-seen still work.

## Checks that distinguish success from easy failures

Save the same examples before and after, then add fresh examples selected without looking at the output.

| Intended outcome | Check | Easy failure it distinguishes |
|---|---|---|
| Duplicate campaigns no longer define topics | Compare centroid-nearest prototypes, raw posts and unique hashes for clusters 2/3/11/15 | Labels look better only because duplicate rows were hidden after fitting |
| Short content is handled by context | Inspect one GM, one emoji reply with parent, one missing-parent reply, one URL-only post and one price/fee bot across Latest, Conversations, Topics and Meaning | A global word threshold silently deletes benign content |
| Author diversity changes the page | Save author run lengths and first-page event IDs before/after; inspect the deferred fill | Results merely reorder ties or return too few posts |
| Near-dedup is conservative | Quote grouped campaign variants and ungrouped price/fee controls | Cosine threshold collapses an entire subject or bot class |
| DBSCAN noise remains descriptive | Browse representative core, boundary and noise posts with `eps`, `min_samples`, raw count and unique-prototype count | Noise is presented as spam or duplicate removal changes density invisibly |
| Semantic retrieval remains useful | Repeat fixed literal/nonliteral Meaning queries and Similar examples | Input filtering improves clusters by emptying semantic search |
| No hidden spend | Compare Titan ledger request/token counts before/after | Context handling silently creates a new paid re-embedding pass |

## Epistemic summary

- *Strong direct evidence*: pinned X/Twitter, Mastodon, Bluesky, and Nostr code paths show feed-specific eligibility, duplicate handling, author diversity, context handling and candidate/ranking separation.
- *Scoped negative finding*: current released X Home Mixer has no declared length filter in its explicit candidate-filter vector. Upstream and unreleased systems remain unknown.
- *Counterevidence to a universal minimum*: older Twitter's only direct word threshold is conditional on surface, network status, language and media/quote context; public defaults do not reveal production values. Mastodon trends has no length check in its eligibility method.
- *Entanglement*: X and old Twitter are related codebases, so they are not independent votes. Mastodon, Bluesky, Nalgorithm and AlgoRelay are independent implementations but solve different surfaces.
- *Hard-to-vary conclusion*: exact duplicate collapse and author diversity directly target the observed repeated-campaign centers and are present as separate stages in several implementations. A minimum-word rule does not target those centers and conflicts with the GM/price/fee controls.
- *Calibrated take*: I assign roughly 0.75–0.9 probability that exact-content prototypes plus slate author diversity improve the inspected feed without needing new embeddings. I assign roughly 0.2–0.4 probability that a universal word minimum improves overall reader utility after accounting for benign short posts. The cheapest way to falsify both estimates is the fixed/fresh example review above.

Full commit, license, source excerpt and search evidence is in [source-inspection.md](source-inspection.md).

-- Pi/gpt-5.6-sol
