# MiniLM topics, spam signals and observed connections

Read-only snapshot of the live SQLite database on 2026-09-13 after the full MiniLM recluster and social-graph import. This uses the actual canonical Primal sets: 143 unique NSFW authors and 13 unique spam authors. It does not use the unsupported 1,590/1,979 figures.

## Graph coverage comes before interpretation

The database had 207,985 edges from 349 observed kind-3 list owners. The configured root followed 109 authors, but only 3 of those 109 had an observed kind-3 list. The other 106 direct follows lacked the contact-list event needed to construct their two-hop paths.

Among 4,920 topic-assigned MiniLM posts from 1,431 authors:

- 11 posts from 4 authors had an observed one-hop path.
- 585 posts from 174 authors had an observed two-hop path.
- 4,324 posts from 1,253 authors had no path in the available graph.

“No observed path” is not evidence that an author is socially unrelated. No path exists in the three stored direct-follow contact lists; newer list updates and paths through the 106 missing direct-follow contact lists remain unknown. The large total edge count mostly belongs to list owners outside this root’s usable one/two-hop graph.

## Spam indicators and connections

Across the 4,920 topic-assigned posts:

- 446 exact same-author/same-text duplicate posts came from 44 authors. Of these, 428 had no observed path and 18 had a two-hop path.
- 34 short posts with more than five URLs came from seven authors. All had no observed path.
- Zero topic authors overlapped the 13-author canonical Primal spam set.
- Zero topic authors overlapped the 143-author canonical Primal NSFW set. This is expected because NSFW-list posts are excluded before topic assignment.

The duplicate and link-farm indicators are enriched among posts without an observed path, but they are not equivalent to social distance. About 88% of all assigned posts have no observed path because graph coverage is sparse. Some duplicate promotions have a two-hop path, and many substantive posts have no observed path. Connections can be a ranking signal; this snapshot does not support using them as a spam decision.

The 143/13 moderation members also have almost no corpus or root overlap: neither set has a stored kind-1 author or a root-direct follow. Thirty-five NSFW members and one spam member occur somewhere among all graph vertices, without stored posts. The canonical lists therefore do not explain the observed duplicate-promotion population.

## Contrasting topic examples

### `conviction · map · trading`

This topic contains 292 posts from 64 authors. It has 140 exact duplicates from eight authors; all 140 have no observed path. The whole topic has 279 no-path posts, 11 two-hop posts and two one-hop posts.

- Duplicate promotion [`055ac937…7041d50c`](http://localhost:8088/context/nostr/055ac93781c402b51415414f5be9faef278c210514b795d21ebfa8527041d50c), no observed path:
  > “We run 4 live trading lanes, $1125 equity, real positions on Alpaca and Robinhood. [...] Pull our book and trade alongside [...] #trading #bots #sylunara”

This is a clear case where semantic/keyword membership, exact duplication and absent observed connection agree. It is not a canonical-Primal-list match.

### `bitcoin · price · sats`

This topic contains 406 posts from 190 authors: 13 exact duplicates, 353 no-path posts, 52 two-hop posts and one one-hop post. Keyword membership mixes substantive Bitcoin discussion with repeated or varied promotions.

- Substantive, two-hop [`00000465…12410084c`](http://localhost:8088/context/nostr/00000465a335e92b127405270f2440fa954b4f4aa2ad405bd84ec0612410084c):
  > “The bitcoin discount is a real signal [...] But a gated community with a pool and a sauna is the opposite of sovereign.”
- Substantive, no observed path [`00008b25…44b44194`](http://localhost:8088/context/nostr/00008b251de4c40ea7b3c180d27fcb8af373d5315c9962e07cdda83444b44194):
  > “Liquid got burned because they were caching their range proofs and doing it wrong. [...] it’s ultimately just like a nocoiner refusing Bitcoin because they hear about a hack.”
- Promotion-shaped but not exact-duplicate-flagged, no observed path [`00001a35…dd4f54c`](http://localhost:8088/context/nostr/00001a3535e3a833bee70b3a29b31867070fd996b1b6df617389b2af2dd4f54c):
  > “Quantum resistance at $150/tx isn’t practical [...] Side note: ETF flows in April ‘26 [...] https://theboard.world/articles/bitcoin-etf-flows-price-dynamics-2026”

The last item is one of 25 posts from 19 authors linking `theboard.world`; all 25 have no observed path and none trigger exact-duplicate or link-farm rules. Their varied wording makes them promotion-shaped, not proven spam. Sixteen fall in the Bitcoin topic, seven in Mixed #6 and two in Mixed #12.

### Security discussion

- Substantive, two-hop [`0000033e…df78cee`](http://localhost:8088/context/nostr/0000033e7f20e0017f5d46b17b1bfcb6acc47f5174bb5b4438bfece77df78cee), Mixed #12:
  > “Monerujo is a solid non-custodial Android wallet [...] install from a source you can verify [...] pair it with a hardware wallet.”
- Directly relevant but no observed path [`0acd1c0f…8282306`](http://localhost:8088/context/nostr/0acd1c0f7c6bc9d7f25e8b66f0a54ca71e8560657a63752b93e5792088282306), Mixed #12:
  > “It is technically possible to recover a lost private key [...] keeping offline backups secure is far more critical.”

These show why connection absence cannot remove security results. The two-hop item has stronger social evidence, while both are semantically on-topic.

## Cluster-input quality follow-up

The current topic fit uses one row per event in `src/embed.rs::cluster_topics`. Every row contributes once to centroid sums. `topic_label` removes duplicate terms within one post, but increments document frequency again for every repeated event. Exact duplicates therefore repeat in both centroid fitting and keyword support. Embedding input comes from `events.content`; stored parent text is rendered for readers but is not part of the child embedding.

The full MiniLM assignment had 4,920 rows but 4,587 unique `(author, content)` pairs: 333 extra duplicate rows (6.8%). This is concentrated rather than uniform. The trading topic had 106 extra rows among 292 (36%); Mixed #3 had 61 among 199 (31%). By contrast, the largest Mixed topic (#1) had only five extra rows among 1,030, while Mixed #2, #6 and #8 had none. Duplicate weighting explains some clusters, not the mostly-Mixed result by itself.

Short contextless replies are also concentrated. There were 138 topic-assigned replies of at most ten characters; 124 had a stored parent. Mixed #0 contained 64 short replies among 244 posts and Mixed #3 contained 28 among 199. But the largest Mixed topic had only 14 among 1,030, and Mixed #6/#8 had none. Missing parent context is a concrete failure family, not a complete explanation of the clustering result.

Concrete cases:

- [`39444b94…d43692e46`](http://localhost:8088/context/nostr/39444b9407679f0cc6f9d3d42292d723dd9a23d01e6fd207abfc8f0d43692e46), Mixed #3, was embedded from the whole input `🤝`. Its stored parent begins, “Allow me to propose an alternative explanation to all of the recent push for AI regulation.” The reader context supplies the AI-regulation subject; the vector cannot. This is missing input information, not evidence that MiniLM misunderstood the supplied text.
- [`b29aee17…11510962e`](http://localhost:8088/context/nostr/b29aee178b63788cce883fcd3e740e14321e02bc2b95f1aa61bd53d11510962e), Mixed #9, was embedded from `bullshit`. Its stored parent discusses recovering a “lost private key” and secure offline backups. Again, Mixed is reasonable for the actual input while the reader sees a security conversation. The failure is missing parent information.
- [`055ac937…7041d50c`](http://localhost:8088/context/nostr/055ac93781c402b51415414f5be9faef278c210514b795d21ebfa8527041d50c), the trading promotion quoted above, appeared five times from the same author in the assigned rows. Across its topic, raw per-event keyword supports were `map=80`, `trading=80`, `conviction=80` against a 59-post threshold. Counting each `(author, content)` once gave support 37 for each against a 38-document threshold in that 4,920-row snapshot. This establishes local threshold sensitivity, not a current global label improvement.

The owner then ran two checks on 12,179 frozen event IDs. A fixed-membership, label-only check changed no memberships. `agents · register` became Mixed and `connecting · connection · service` became `connecting · connection · error`; the trading label did not change. Thus the tested keyword-vote deduplication did not improve the suspected trading label and lost another descriptive label.

The full-fit report says it removed 567 repeated weights from 12,179 rows, leaving 11,612 unique `(author, content)` pairs. A direct SQL recount of its frozen baseline database finds 11,059 such pairs, leaving 1,120 repeats; this agrees with the later label-only report and contradicts the full-fit report. The temporary full-fit source patch was not preserved. Therefore its exact treatment key and strength are unresolved, and its membership changes cannot yet be attributed specifically to full `(author, content)` deduplication.

The resulting alternate clustering still supports a bounded outcome comparison: after Hungarian alignment, 3,059 events (25.12%) moved. Membership churn and six versus five descriptive labels are not quality measures because the baseline is not ground truth and the labels use a heuristic threshold. The initial-centroid log records the exact same 16 event IDs in both full-fit runs. The churn arose after iterative centroid updates, not from different initialization; which repeated rows caused those update differences remains unresolved by the count mismatch.

Inspection of moved posts shows mixed quality effects:

- News separation improved. Deduplicated topic 8 was 71.3% posts matching `#News`, `news.netasgard.com` or Flickr-news patterns, versus 5.3% in baseline topic 8. It gained 618 posts from baseline topic 15, including [`00e2ae33…b271575d`](http://localhost:8088/context/nostr/00e2ae33a3cb338811e9c40e9e71c71dc473c4df23c4a905ac976c56b271575d), “Over 100 rescued from Indonesian vessel after storm hits #News [...] Photo [...] Flickr,” and [`596fa467…df6ef78b`](http://localhost:8088/context/nostr/596fa4673985892a0d12415e923969a28dd8789d452d59e1911c698bdf6ef78b), “Surge in road deaths [...] Source: Observer.” The remaining deduplicated topic 15 fell from 44.6% to 16.9% news-pattern posts and was honestly labelled Mixed. This looks more useful, though topic 8 still contains travel/photo prose.
- A machine-log group became visible. Deduplicated topic 11 was 63.7% `nlogpost:` messages, versus none in baseline topic 11. [`0414f38f…929ee2`](http://localhost:8088/context/nostr/0414f38f82100afd2bcd854d0c5d8e2f02c66e48fbd6514ddf6a705d971fc5e4) is `nlogpost:1789295844:[[[[p-slime shots build #0]]]]`. The cluster is not clean: it also contains “I'm guessing Cold Card RNG and Liquid” and paper links.
- The trading group became less coherent. Baseline topic 13 had 323 posts and 90.1% matched Sylunara/trading-promotion patterns. Deduplicated topic 13 had 809 posts but only 36.0% matched those patterns. It absorbed [`000001ec…b0b6d3b`](http://localhost:8088/context/nostr/000001ec977bdd21cc908d26605b57fc06772504d2085e76d91f5997cb0b6d3b), “DHL rejected my API key request [...] Agent [...] packages,” and [`0000033e…df78cee`](http://localhost:8088/context/nostr/0000033e7f20e0017f5d46b17b1bfcb6acc47f5174bb5b4438bfece77df78cee), the Monerujo/hardware-wallet security post. This is worse for browsing a trading topic even though its label changed from `sylunara` to the less specific `live`.

The alternate clustering therefore has plausible improvements and clear regressions. Production promotion is held because net reader benefit and the full-fit treatment identity are unverified, not because duplicate weighting is disproven. Production and live clusters remain unchanged. Parent-context embedding remains a separate possible experiment because changing both weights and inputs would not identify which change mattered. See the corrected `/workspace/meatybroth-rust/slop/research/2026-09-13_unique-author-content_topic_weighting.md` at `61056d5` and the label-only report at `3e73673`.

### Discriminative label-only follow-up

A separate fixed-membership experiment ranked terms by within-topic support times global inverse document frequency, with a floor of 2% of topic posts (at least five) and three authors. It changed no assignments and labelled 15 of 16 topics rather than six.

Some labels are useful:

- `bitcoin · crypto · money`: the terms cover 28.5%, 14.2% and 11.8% of its 954 posts and come from 158, 55 and 62 authors. The three newest samples include a market-position post, an open Bitcoin hardware-wallet promotion and a Bitcoin/gold argument.
- `trading · trade · signals`: the terms cover 90.7%, 77.7% and 55.7% of 323 posts. This accurately names the repeated trading campaign, although each term comes from only 3–6 authors; it should not be read as a broad community topic.
- Existing `bitcoin · price · block` and `news · photo · flickr` remain clear from their samples.

Several promoted labels overstate a small or incoherent subset:

- `morning · day`: `morning` occurs in 5.1% and `day` in 2.5% of 565 posts. Its newest samples discuss treadmill shoes, a baby bot and Quebec financial support; none substantively concerns a morning/day topic.
- `nostr · 70u · news`: `70u` and `news` occur in only 2.4% and 3.1% of 910 posts. Samples are a cricket match, Greek trending tags and a `2:40` timestamp.
- `agent · tech · data`: its terms cover only 5.9–7.6% of 641 posts. Two samples are SEO hashtag/link spam and the third advertises 2,000 AI analysts. The label launders promotion patterns into a generic technical topic.
- `yas · weekend · marina`: each term covers 14–16% of 322 posts, but newest samples concern flights, learning to cook and driving from Dubai. This may identify a Yas Marina travel subgroup; it does not summarize the full cluster.
- `people · life · true` is too generic to help choose a topic even where the words occur.

The three-sample excerpts are bounded newest-post checks, not random quality estimates. Full-cluster support confirms the weakest cases independently: the 2% floor permits labels where 95–98% of posts do not contain a displayed term. The interface shows no confidence or coverage, so a reader cannot distinguish `trading` at 91% support from `day` at 2.5%.

Recommendation: reject global promotion of this label rule. False semantic names are worse than explicit Mixed labels. The clear wins do not establish a single honest automatic criterion, and manually allowing them would create a maintenance list. A later criterion should be judged against blinded samples and must require evidence that the displayed words summarize the cluster rather than a distinctive minority; the current 2%/three-author rule does not. Live labels correctly remain unchanged.

## Result

Observed connections add useful evidence for some results, especially when they agree with duplicate/link-farm indicators. Current root-graph coverage is too incomplete to treat no-path posts as nonconnections or spam. Topic keywords isolate a highly duplicated trading-promotion group, but the Bitcoin and security examples show that topic membership and social distance each mix legitimate and promotion-shaped content. The canonical 143/13 author lists have zero overlap with the topic corpus and do not resolve this classification problem.

## Evidence

- `topics-social-analysis.json`: per-topic counts, relation counts, warning counts and quoted candidates
- `/workspace/meatybroth/slop/reviews/topics-social/analyze_topics_social.py`: temporary reproducible read-only query outside the sibling Rust repository
- `moderation-graph-overlap.txt`: canonical moderation-list overlaps
- `topics-social-example-http.tsv`: all connection-analysis context URLs returned HTTP 200
- `cluster-input-weighting.json`: duplicate, short-reply, label-support and parent-text reconstruction
- `short-reply-parent-candidates.json`: bounded reply/parent sample
- `cluster-input-examples-http.tsv`: all three cluster-input examples returned HTTP 200
- `/workspace/meatybroth-rust/slop/research/2026-09-13_unique-author-content_topic_weighting.md`: corrected disputed-treatment report at `61056d5`
- `topic-dedup-flow-matrix.txt`, `topic-dedup-moved-samples-02.txt`, `topic-dedup-coherence-counts.tsv`: moved-post and pattern-composition review
- `topic-dedup-moved-http.tsv`: quoted moved-post context URLs returned HTTP 200
- `/workspace/meatybroth-rust/slop/verification/2026-09-13_topic-label-unique-comparison.log`: fixed-membership duplicate-vote control
- `/workspace/meatybroth-rust/slop/verification/2026-09-13_topic-discriminative-comparison.log` and `2026-09-13_topic-discriminative-fixed-samples.log`: fixed-membership global-IDF labels and newest samples
- `topic-discriminative-label-support.tsv`: full-cluster support and author counts for selected labels
- `/workspace/meatybroth-rust/slop/verification/2026-09-13_topic-initial-centroid-identities.log`: all 16 initial event IDs match
- `topic-frozen-unique-count-check.txt`: direct 12,179-row baseline recount gives 11,059 unique `(author, content)` pairs

-- Pi/gpt-5.6-sol
