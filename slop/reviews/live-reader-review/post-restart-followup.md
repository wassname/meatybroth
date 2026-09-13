# Integrated reader follow-up

Reviewed the owner’s integrated `3ea764b` process at `http://localhost:8088/` after the 15:34 UTC readiness notice. This supplements `current-review.md`; it does not erase the before-restart evidence.

## Fixed in the running app

- The root now says “Live SDK collection and reader.” Status timestamps advanced with the review.
- New and Social each returned 100 posts. Social returned in 0.66 s in my sample and now visibly includes two-hop results. Before restart it had 12 one-hop posts and took 2.80 s.
- Presence telemetry no longer occupies the New feed.
- Post-restart event [`5ead12…456489` Similar](http://localhost:8088/?mode=similar&similar=nostr:5ead12eda0399cf3200955d7dfbef8aa545ee6986f254515500172ae86456489&embedding=minilm) returned 100 results and now has a clear “Similar posts” heading plus a source-post/thread-context link. The Feed selector also says Similar.
- Status now renders per-space backend, model, dimensions, revision/hashes, stored/pending vectors, completed requests, tokens, recorded cost and uncertain requests.
- Titan Topics now states: “Titan has 119 cached posts. This partial cohort is proof-of-work-biased; browsing does not call AWS.” This resolves the earlier undisclosed-corpus limitation.
- Topic labels no longer claim meaning for low-coherence clusters.

## Remaining blockers

1. Telemetry was hidden from browsing but stale derived data remained after startup. `3516c300…b51995` (`zone_presence`) and `56e0a2a9…afbf53` (`presence`) each returned HTTP 404 from context but HTTP 200 with 100 MiniLM Similar results. Later timestamp reconstruction showed both were received at 15:26:47 UTC and embedded at 15:28:00 UTC, before the reviewed integrated runtime. This proves missing startup cleanup and a missing Similar source-eligibility check; it does not prove the integrated live incoming path embedded them.
2. Honest topic labels are not yet usable navigation. MiniLM shows ten separate links named only `mixed`; Titan shows four. Counts differ, but a reader cannot identify a specific mixed cluster by name. A visible topic number would distinguish them without inventing a semantic label; it need not persist across reclustering.
3. Updated embedding counts are not self-explanatory. The sampled status showed `bedrock ... 119 eligible / 119 stored · 6594 eligible pending` and `minilm ... 4940 eligible / 5799 stored · 1773 eligible pending`. “Stored” exceeds “eligible” for MiniLM, and “eligible” appears to have different scope from “eligible pending.” A fresh reader cannot reconstruct the definitions.
4. Semantic ranking remains noisy. The new Similar example concerns local sports trends, but its first two cards are an identical duplicate-content promotion. Later results about football streams and a Latin-American sports site are recognizably related. The fixed local-LLM query still returns useful local execution/model-software results with unrelated posts mixed in. The private-key query remains useful but includes a botnet/AI post near the top.

## Fresh screenshot reading

I opened every PNG after capture.

- `09-home-after-integrated.png`: the prior raw presence JSON is gone. The first cards are normal posts, though missing profiles, AI-generated prose and duplicate-content warnings remain common.
- `10-similar-new-after-integrated.png`: the page purpose and source link are now obvious. The top two result cards are duplicate spam; later sports results are related to the source.
- `11-status-after-integrated-top.png`: the live SDK claim and fresh per-relay timestamps are visible. The page still places 100 gap rows before embedding information.
- `12-status-after-integrated-embeddings.png`: model provenance and accounting are readable after scrolling, but the eligible/stored/pending terminology is ambiguous as described above.
- `13-social-after-integrated.png`: a full recent feed with explicit `2 hops · connection 2.00` labels, profile/address examples, replies and text-only media links.
- `14-topics-minilm-after-integrated.png`: most links read only `mixed`, so the sparse page gives little basis for choosing among them.
- `15-topics-titan-after-integrated.png`: the 119-post PoW limitation is clear; four of six topics are indistinguishable `mixed` links.

## Evidence

- `post-restart-http-timings.tsv`
- `post-restart-browser.jsonl`
- `post-root.html`, `post-status.html`, `post-new-similar.html`
- screenshots `09-*.png` through `15-*.png`

-- Pi/gpt-5.6-sol
