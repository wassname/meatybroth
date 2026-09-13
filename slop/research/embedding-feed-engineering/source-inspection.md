# Source inspection record

Inspected 2026-09-13. Repositories were cloned read-only for source browsing and were not executed. Line links below pin the inspected commits.

## Search coverage

Searches varied across:

- current X/Phoenix candidate retrieval, ranking, filters, duplicates, authors, seen history and semantic IDs;
- older Twitter Home Mixer and push-notification quality filters;
- Bluesky feed generators, semantic/embedding feeds and runnable demos;
- Nostr recommendation, relevance, replies, boosts, author mixing and recency;
- production/open-source eligibility and trend ranking (Mastodon);
- news/semantic recommendation and near-duplicate/diversity implementations.

GitHub searches that produced useful candidates included `bluesky feed generator`, `bluesky semantic feed`, `atproto embedding feed`, `nostr recommender`, `nostr recommendation`, `semantic news recommender`, and `news embeddings recommender`. The embedding-specific GitHub searches were sparse and mostly returned zero-star prototypes. This is evidence about those searches, not proof that no other implementation exists.

## Repository snapshots

| Repository | Inspected commit | Last commit | Stars at inspection | License |
|---|---|---:|---:|---|
| [xai-org/x-algorithm](https://github.com/xai-org/x-algorithm) | `6bb4594253cdfa9ea19983a54a401d5ce8f8275d` | 2026-09-12 | 33,178 | Apache-2.0 |
| [twitter/the-algorithm](https://github.com/twitter/the-algorithm) | `c54bec0d4e029fe34926ef3258a86ccacc0d0182` | 2025-09-03 | 73,907 | AGPL-3.0 |
| [mastodon/mastodon](https://github.com/mastodon/mastodon) | `a7cc70db674d79616597fcc1e67aafee6fcb2f11` | 2026-09-13 | 50,299 | AGPL-3.0 |
| [bluesky-social/feed-generator](https://github.com/bluesky-social/feed-generator) | `9224e985f7fa75b6f3ebfb83c98db514a28fb875` | 2026-06-12 | 2,064 | MIT |
| [MarshalX/bluesky-feed-generator](https://github.com/MarshalX/bluesky-feed-generator) | `be500ba5be2c2006f0649c8ce8862943ac7966c3` | 2026-08-20 | 304 | MIT |
| [jooray/nalgorithm](https://github.com/jooray/nalgorithm) | `bbacb1e0161e8bd690c38c65a31df90cd5e15d5b` | 2026-09-08 | 3 | no detected license |
| [barrydeen/algo-relay](https://github.com/barrydeen/algo-relay) | `0c49f8597dc78534bfab8c17ac68b87306ccccc9` | 2025-02-28 | 54 | MIT |
| [Princeton-HCI/cos-atproto-pds](https://github.com/Princeton-HCI/cos-atproto-pds) | `9aad1247e52349f27f3b36468d07d411fccad16a` | 2026-02-08 | 0 | MIT |
| [drbh/bsky-feeder](https://github.com/drbh/bsky-feeder) | `7b51b85459b6f752f83d15ffa0cb92d98ea53f3d` | 2024-11-18 | 3 | no detected license |

Stars are context only, not quality measurements. X describes Phoenix as production code but omits production data, checkpoints and orchestration. Princeton calls its repository a research project. Nalgorithm is very recent and low-adoption. The Bluesky starter is infrastructure rather than a recommendation algorithm.

## Scoped negative inspection: short-post filtering in X

Current X Home Mixer declares its candidate filters in one explicit vector. It includes ID duplicate removal, age, out-of-network reply/retweet filtering, NSFW clusters, seen/served history, social graph and engagement filters. It does not declare a text-length or word-count filter:

- [Phoenix filter list, lines 354–381](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/home-mixer/candidate_pipeline/phoenix_candidate_pipeline.rs#L354-L381)
- [post-selection visibility and conversation filters, lines 437–441](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/home-mixer/candidate_pipeline/phoenix_candidate_pipeline.rs#L437-L441)

This supports only: *the inspected current Home Mixer filter path has no length filter*. It cannot establish what upstream corpus construction, semantic-ID generation, unshipped safety systems or current production configuration does.

The older Twitter source has a real length predicate, but it is for eligible out-of-network push candidates, not the Home feed. It chooses different thresholds by language and presence of media/quote, has a hard-coded five-word case, strips URLs before word counting, and runs only when `enableFilter` is true:

- [context-dependent word thresholds and hard-coded 5, lines 51–77](https://github.com/twitter/the-algorithm/blob/c54bec0d4e029fe34926ef3258a86ccacc0d0182/pushservice/src/main/scala/com/twitter/frigate/pushservice/predicate/OutOfNetworkCandidatesQualityPredicates.scala#L51-L77)
- [`enableFilter` condition, media/quote context and comparison, lines 78–128](https://github.com/twitter/the-algorithm/blob/c54bec0d4e029fe34926ef3258a86ccacc0d0182/pushservice/src/main/scala/com/twitter/frigate/pushservice/predicate/OutOfNetworkCandidatesQualityPredicates.scala#L78-L128)
- [URL-stripped word count](https://github.com/twitter/the-algorithm/blob/c54bec0d4e029fe34926ef3258a86ccacc0d0182/pushservice/src/main/scala/com/twitter/frigate/pushservice/predicate/PostRankingPredicateHelper.scala#L44-L48)
- [open-source defaults: disabled and zero thresholds](https://github.com/twitter/the-algorithm/blob/c54bec0d4e029fe34926ef3258a86ccacc0d0182/pushservice/src/main/scala/com/twitter/frigate/pushservice/params/PushFeatureSwitchParams.scala#L3724-L3801)

The actual configured production thresholds and language sets are absent/redacted in this snapshot. Emoji-only text is not explicitly detected in this path. It would be counted by whitespace after URL removal; this is code inference, not an author claim.

## Verbatim source excerpts

### Phoenix README

> Phoenix is a recommendation system that predicts user engagement (likes, reposts, replies, etc.) for content. It operates in two stages:
>
> 1. **Retrieval**: Efficiently narrow down millions of candidates to hundreds, scoring a user embedding against a precomputed candidate index
> 2. **Ranking**: Score and order the retrieved candidates using a more expressive transformer model

Source: [xai-org/x-algorithm Phoenix README, lines 38–45](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/phoenix/README.md#L38-L45). This is xAI's description of its own release.

> **Candidate Tower**: Computes normalized embeddings for all items in the corpus `[N, D]`. Since the semantic-ID migration, candidates are represented by their **semantic IDs** — residual-quantized codes (6 levels × 256 codes) derived from each post's multimodal embedding — plus hashed author IDs, rather than by hashed post IDs alone.

Source: [Phoenix README, lines 109–116](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/phoenix/README.md#L109-L116). This is not a recipe for raw Titan text-vector clustering.

> This walkthrough verifies the shipped nano ranking and retrieval paths with synthetic data: train, checkpoint, resume, serve, then send a retrieve → rank request. It is not a production-quality model or a production-scale setup. Production data, checkpoints, orchestration, and scale are not included.

Source: [Phoenix quickstart, lines 3–7](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/phoenix/QUICKSTART.md#L3-L7). This bounds what its public demo proves.

### X/Twitter duplicate and diversity code

> `fn diversity_multiplier(decay_factor: f64, floor: f64, exponent: f64) -> f64 {`
> `    (1.0 - floor) * decay_factor.powf(exponent) + floor`
> `}`

Source: [current X ranking scorer, lines 625–627](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/home-mixer/scorers/ranking_scorer.rs#L625-L627). The exponent is the author's position among candidates sorted by pre-diversity score ([lines 629–646](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/home-mixer/scorers/ranking_scorer.rs#L629-L646)).

> `if seen_ids.insert(candidate.tweet_id) {`
> `    kept.push(candidate);`
> `} else {`
> `    removed.push(candidate);`
> `}`

Source: [current X exact ID filter, lines 14–20](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/home-mixer/filters/drop_duplicates_filter.rs#L14-L20). This is event-ID deduplication, not text/campaign deduplication.

> `case (Some(c95), _) if historicalClusters.contains(c95) =>`
> `  // Historical match at 0.95 threshold, remove it`
> `case (Some(c95), _) if seenCluster95Ids.contains(c95) =>`
> `  // Duplicate at 0.95 threshold (non-historical), remove it`

Source: [older Twitter cluster-based dedup, lines 76–104](https://github.com/twitter/the-algorithm/blob/c54bec0d4e029fe34926ef3258a86ccacc0d0182/home-mixer/server/src/main/scala/com/twitter/home_mixer/functional_component/filter/ClusterBasedDedupFilter.scala#L76-L104). The same file also has a looser `0.88` cluster ID. These are named feature thresholds in code; the inspected file does not show how cluster IDs were computed.

### Context and replies

> `(c.in_network == Some(false) && (is_retweet || is_reply))`
> `    || (is_reply && c.ancestors.is_empty())`

Source: [current X out-of-network reply/retweet filter, lines 13–18](https://github.com/xai-org/x-algorithm/blob/6bb4594253cdfa9ea19983a54a401d5ce8f8275d/home-mixer/filters/oon_retweet_reply_filter.rs#L13-L18). Out-of-network replies are removed; in-network replies with ancestors can survive this filter.

> - **Quote posts** (kind 1 with `nostr:` references) -- scores the quote + embedded post together
> - **Boosts** (kind 6) -- resolves and scores the original content
> - Replies are filtered out
> - Plain boosts of the same note are merged into one card ("alice, bob and carol boosted it")

Source: [Nalgorithm README, lines 15–22](https://github.com/jooray/nalgorithm/blob/bbacb1e0161e8bd690c38c65a31df90cd5e15d5b/README.md#L15-L22). The implementation filters replies at [fetcher lines 248–251](https://github.com/jooray/nalgorithm/blob/bbacb1e0161e8bd690c38c65a31df90cd5e15d5b/lib/src/fetcher.ts#L248-L251). This is a project design choice, not production consensus.

### Independent feed implementations

> `// Distribute one note per author across all variants`
> `for _, notes := range authorNotes {`
> `    for i := 0; i < len(notes) && i < numFeedVariants; i++ {`

Source: [AlgoRelay author distribution, lines 177–197](https://github.com/barrydeen/algo-relay/blob/0c49f8597dc78534bfab8c17ac68b87306ccccc9/algorithm.go#L177-L197). It produces five cached variants and rotates them on requests ([lines 140–158](https://github.com/barrydeen/algo-relay/blob/0c49f8597dc78534bfab8c17ac68b87306ccccc9/algorithm.go#L140-L158)). It is not embedding-based.

> `MAX_PER_AUTHOR = 10 # max posts per author in a feed`
> `MAX_AGE_SECONDS = 48 * 60 * 60  # 48 hours in seconds`

Source: [Princeton HCI Bluesky feed prototype, lines 16–21](https://github.com/Princeton-HCI/cos-atproto-pds/blob/9aad1247e52349f27f3b36468d07d411fccad16a/bluesky-feed-manager/server/algos/feed.py#L16-L21). It merges text and vector candidates, deduplicates by URI and then applies the author cap ([lines 381–451](https://github.com/Princeton-HCI/cos-atproto-pds/blob/9aad1247e52349f27f3b36468d07d411fccad16a/bluesky-feed-manager/server/algos/feed.py#L381-L451)). Its final relevance scorer is keyword/profile based, so embeddings are candidate generation rather than the complete ranker.

> `if config.IGNORE_REPLY_POSTS and record.reply:`
> `    logger.debug(f'Ignoring reply post: {uri}')`
> `    return True`

Source: [maintained Bluesky Python template, lines 29–41](https://github.com/MarshalX/bluesky-feed-generator/blob/be500ba5be2c2006f0649c8ce8862943ac7966c3/server/data_filter.py#L29-L41). The same ingest-time filter optionally rejects posts whose embedded creation date is over a day old, while its comment explicitly warns that outages can make this rule discard legitimate delayed events ([lines 12–26](https://github.com/MarshalX/bluesky-feed-generator/blob/be500ba5be2c2006f0649c8ce8862943ac7966c3/server/data_filter.py#L12-L26)).

> `status.created_at.past? &&`
> `  opted_into_trends?(status) &&`
> `  !sensitive_content?(status) &&`
> `  !status.reply? &&`
> `  valid_locale?(status.language)`

Source: [Mastodon trending-status eligibility, lines 91–98](https://github.com/mastodon/mastodon/blob/a7cc70db674d79616597fcc1e67aafee6fcb2f11/app/models/trends/statuses.rb#L91-L98). This production open-source path separates trend eligibility from storage/display; it does not impose a text-length test.

## Demo checks

HTTP checks on 2026-09-13:

- `https://cypherpunk.today/nalgorithm/` returned 200. This is Nalgorithm's linked live Nostr demo. It requires the user's LLM/provider choice for scoring.
- `https://skyfeed.app/` returned 200. It is a public visual Bluesky feed builder; it is useful for trying rule-based feed composition, but its implementation was not treated as open-source evidence.
- `https://bsky.app/` returned 200. Generated feeds can be consumed in the normal Bluesky client.
- The official Bluesky starter documents a local runnable feed endpoint and provides firehose/indexing scaffolding, but its supplied `whats-alf` example is a keyword feed, not semantic ranking: [README](https://github.com/bluesky-social/feed-generator/blob/9224e985f7fa75b6f3ebfb83c98db514a28fb875/README.md#L1-L46).
- Phoenix provides an end-to-end synthetic quickstart, but requires Linux, CUDA 12, Python/Rust/protoc and model training. It was not executed because browsing did not require running untrusted code and its own documentation says the result does not demonstrate recommendation quality.

-- Pi/gpt-5.6-sol
