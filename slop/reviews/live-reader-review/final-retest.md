# Final localhost retest

Reviewed the owner’s replacement runtime on `http://localhost:8088/` at 15:46–15:51 UTC. I did not restart it.

## Previously blocking outcomes

- `3516c300…b51995` (`zone_presence`) and `56e0a2a9…afbf53` (`presence`) remain in the canonical SDK event store, as required for one source of truth.
- Both are absent from reader eligibility and now have zero post vectors, zero embedding chunks and zero topic memberships.
- Both context URLs return HTTP 404 in 0.026–0.029 s.
- Both MiniLM Similar URLs return HTTP 400 in 0.26–0.29 s with the visible message “The source post is not eligible in the current reader window.” There is no semantic fallback.
- Timestamp correction: both events were created at 15:24:56 UTC, received at 15:26:47, and had been embedded at 15:28:00. They predated the integrated runtimes reviewed later. The observed defect was stale derived-data cleanup plus a missing Similar source-eligibility check, not evidence that the later live incoming path embedded them.
- The non-telemetry JSON price card `84c9f33…d8d7cb4` remains reader-eligible with a MiniLM vector. Its context and Similar URLs both return HTTP 200. This is a live counterexample to blanket JSON filtering. The quoted-prose counterexample is maintained-test evidence, not present in the sampled live corpus.
- Topics now identify low-coherence groups as `Mixed #<topic id>`, so readers can distinguish them without invented semantic labels. The ID is not guaranteed to persist across reclustering.
- Status now distinguishes “eligible posts embedded,” “eligible posts pending,” and “total stored vectors, including posts no longer eligible.” MiniLM counts sum to the same eligible corpus as Titan: 5,030 embedded + 3,059 pending; Titan 119 + 7,970.

## Fresh visual reading

I opened each PNG after capture.

- `16-final-status-embeddings.png`: moderation event IDs and dates, complete model provenance, clear embedded/pending wording, request/token/cost accounting and zero uncertain requests are visible. The page still requires scrolling through many collection-gap rows.
- `17-final-topics-minilm.png`: repeated low-coherence topics are now distinguishable (`Mixed #1`, `Mixed #8`, and so on). Four keyword labels remain for the more coherent groups.
- `18-final-telemetry-similar-error.png`: the red eligibility error is immediately visible, Feed says Similar, and no result cards render. The generic source/thread link remains even though that context URL is intentionally unavailable.

## Remaining reader-quality limitations

- MiniLM Meaning and Similar are useful in several inspected results, but duplicate promotions and unrelated posts still reach top positions. Cosines do not establish quality.
- Most MiniLM topics are honestly labelled Mixed rather than providing meaningful topic names.
- The status page is accurate but long; embedding information appears after 100 individual gap rows.
- The moderation audit found that status counts 157 NSFW list entries although only 143 pubkeys are unique. See `moderation-source-distinction.md`.

## Evidence

- `final-retest-http.tsv`
- `final-telemetry-cleanup-sql.txt`
- `final-json-price-http.tsv`
- screenshots `16-final-status-embeddings.png` through `18-final-telemetry-similar-error.png`

-- Pi/gpt-5.6-sol
