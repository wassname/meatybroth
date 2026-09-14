# Similar-replies parent diversity

## Selection rule

The context-only recommendation list keeps the highest-ranked candidate for each known direct parent/quoted-source ID. Top-level candidates use their own event ID, so unrelated top-level posts remain distinct. Existing thread IDs remain excluded before diversity selection.

The process retrieves at most 64 unshown cached candidates and returns at most five. It does not add a threshold, score, configuration field, moderation rule, embedding request, or provider call. The normal Similar result page and actual thread replies are unchanged.

## Regression

The pure selection regression supplies five score-ordered candidates with one parent, one candidate with another parent, and two top-level controls. It verifies output `[first same-parent, other-parent, top-level-1, top-level-2]` in original score order. The existing SDK integration regression still verifies current/reply/related ordering, unique rendering, and unchanged embedding-provider call count.

## Verification

- `slop/verification/2026-09-14_similar-diversity-tests.log`
- `slop/verification/2026-09-14_similar-diversity-clippy.log`
- `slop/verification/2026-09-14_similar-diversity-release.log`

## Local observations

The final HTTP checks found:

- saved example: 31 actual thread cards, then the heading, then 5 recommendations; all 36 article IDs unique; all 5 direct-parent/source keys distinct; Titan visibly selected;
- fresh example: 26 actual thread cards, then the heading, then 5 recommendations; all 31 article IDs unique; all 5 keys distinct; Titan visibly selected.

The saved example's former five-candidate `db76…` group was reduced to its highest-ranked member. This fixed repetition, not general retrieval quality. Terra judged only one of its five final recommendations directly relevant. On the fresh CPI/tech-exposure example, Terra judged three plausibly useful and two clear misses. This is selection diversity, not spam exclusion or an aggregate quality result.

Terra's independent signed review is `slop/reviews/live-reader-review/similar-replies-diversity-review.md`. Local machine output is `slop/verification/2026-09-14_similar-diversity-final-http.log`.

-- Pi/gpt-5.6-sol
