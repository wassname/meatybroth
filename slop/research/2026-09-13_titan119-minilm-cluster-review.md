# Partial PoW-biased Titan/MiniLM cluster review

Date: 2026-09-13. Status: bounded offline review of the 119 cached Titan posts, not a retained-corpus Titan result. The cohort is severely selected for NIP-13 proof-of-work notes and cannot support a corpus-level model comparison. No post was hidden or deleted, and clustering settings were not changed.

## What exists

The interrupted one-off run saved 119 normalized Titan V2 512-dimensional post vectors in space `64c3c581…9703bbf5`: 7,247 billed input tokens and 144,940 nano-USD (**$0.00014494**). The same 119 event IDs have MiniLM 384-dimensional vectors in space `bd46d8b3…16db644e`.

The Titan rows are the deterministic lowest-ID pending slice because production selects `ORDER BY e.id`. This is not a random hash slice. All 119 Titan IDs begin with four zero hex digits and all 119 events have a `nonce` tag; among 6,381 eligible non-Titan notes, only 262 begin with four zero hex digits and 267 have a nonce tag. [NIP-13](https://github.com/nostr-protocol/nips/blob/master/13.md) states that proof-of-work difficulty is “the number of leading zero bits in the NIP-01 id” and miners update a nonce before recalculating the ID. Ordering by ID therefore selects mined notes first. The 119 posts are a specific PoW cohort and only 4.1% of the earlier 2,874-post MiniLM set.

Titan topic rows do not exist because authentication failed before full backfill and final clustering. For diagnosis only, I applied the current deterministic cosine clustering procedure separately to the 119 Titan and matched MiniLM vectors: six clusters, farthest-first initialization, eight assignment/update passes and document-frequency labels. This did not write to SQLite.

## Useful and weak results

Titan separated two recognizable groups:

- `supply · chain · china` (20 posts): centroid examples discuss Myanmar ore/ports and mineral supply constraints. Example `000010a3…22e26cf` begins, “THREAD: Myanmar 2026: China vs India for Ore and Ports”.
- `all · numbers · people` (10 posts): several posts concern a 2,100-receipt zap analysis. Example `000008d9…f03ecc4` begins, “The concern is fair and the numbers back it up. I pulled 2,100 zap receipts…”

Other groups are not yet useful feed topics:

- `most · now · acting` mixes an image-only URL, a sentence about feminism and US/Houthi politics.
- `cards · cut · keep` has only four posts and mixes payment privacy with diet advice.
- `good · year · all` is largely short “GM” posts. It is coherent as a low-information style cluster, but not a useful subject label.
- `them · because · don` is a heterogeneous remainder.

MiniLM also isolates the supply-chain group, but its other groups are broad or style-driven. Within this PoW cohort, only 50/119 exact nearest neighbors agree. The median rank of Titan's nearest neighbor in MiniLM space is 3, but the mean is 17.5; adjusted Rand index between the two six-cluster assignments is 0.058. These mechanics show that the spaces differ on mined notes. They do not say Titan is better and must not be extrapolated to ordinary notes.

## Disagreements worth retaining

The examples were selected from deterministic nearest-neighbor disagreements, not used to tune either model.

- Anchor `0000003a…54a10dd` is a rap-like line about police and drugs. Titan retrieves another drug/police lyric (`000000c4…d3c3cb3a`); MiniLM retrieves a studio-time lyric (`0000016c…801eee3`). Both are recognizable, with Titan closer on subject.
- Anchor `00000077…b4c0d87` is “why? ai?”. Titan retrieves a question about open-source AGI (`00000dc2…134ac92`); MiniLM retrieves an AI-doomer discussion (`000001cc…68da6`). Both are defensible; the anchor is too short to choose confidently.
- Anchor `00000014…a3bc22` discusses childish behavior and accountability. MiniLM retrieves a reflection on judging the version of themselves people can afford to show (`000002b3…628e1`); Titan retrieves “I usually skip this part” (`00000b7c…55e6a9`). Titan's result is garbage for this anchor.
- For an opaque YouTube URL (`00000008…d378652`), Titan returns a K-pop stream while MiniLM returns a BMW post. Neither model has enough text; both results are garbage. This is an input problem rather than evidence for a model choice.

## Spam and social signals do not resolve these clusters

The saved Primal spam list has 1,979 pubkeys and the NSFW list has 1,590. None of the 119 posts is by a listed spam author. None matches the existing explicit-content phrase rule or the existing short-link-farm rule. A fixed exploratory marker set (`spam`, `scam`, `bot`, `airdrop`, `giveaway`, `casino`, `onlyfans`, `nsfw`, `telegram`) also matches zero. This selected sample therefore contains no positive spam-list/keyword comparison; zero overlap is not evidence that a cluster is clean.

The social graph is much less complete than the post set. The stored root kind-3 event has 109 direct follows, but only one direct follow has a stored kind-3 list from which a second hop can be observed. Across all 934 MiniLM-topic authors, two are observed direct follows and zero are observed at two hops; none of those two appears in this 119-post slice. “Not observed connected” mostly means missing graph metadata, not a known social nonconnection. The root event itself was created 2026-01-25, although it was fetched again today.

Consequently, spam-list overlap, keyword rules and observed 1/2-hop membership cannot justify automatic suppression here. Labels are frequent words, not safety judgments. The full Titan run needs primary samples that include listed authors, connected authors and disagreements before these indicators can be compared.

## Epistemic status and next comparison

Observed: exact cached IDs/vectors/tokens/cost, 119/119 PoW selection, deterministic cluster assignments, primary post text, moderation-list sizes and stored graph coverage. Inference: supply-chain and zap groups are recognizable within the mined cohort; several other groups are weak. Opinion: the 119-post result is not useful enough to publish as the production topic feed or compare model quality across the retained corpus.

The full retained-corpus comparison should repeat this exact review without tuning: centroid representatives for every topic, deterministic neighbor disagreements, list/rule counts, observed graph coverage and benign same-topic counterexamples. Actual user review should judge useful versus garbage posts. Counts and agreement statistics are diagnostics only.

[Raw cluster assignments, examples and signal counts](../verification/2026-09-13_titan119-minilm-offline-analysis.log)

-- Pi/gpt-5.6-sol
