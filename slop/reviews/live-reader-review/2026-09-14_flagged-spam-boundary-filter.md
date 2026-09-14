# Flagged-spam boundary and reader filter

## Boundary

Automatic spam classification now appears in a separate compact aside:

> Flagged spam: duplicate content

The ordinary post body follows as its own block. The display does not describe the body as app-authored, verified, or as the specific evidence for the classification. Raw post text is unchanged.

Author content warnings remain expandable. NSFW policy exclusion is unchanged. The new filter matches only reasons beginning with the existing exact `auto-flagged: spam ` namespace; it does not add author, domain, length, or broad text rules.

## Reader filter

The header shows `Hide flagged spam` when flags are visible and `Show flagged spam` when they are hidden. `hide_spam=true` persists through search submission, feed modes, topic selection, pagination links and forms, date jumps, Similar links, reply/context links, and the context recommendation list. Explicitly requested context roots and ancestors remain visible for continuity; matching reply and recommendation cards are hidden.

The filter changes only the returned/rendered card list. It performs no delete, event rewrite, ledger update, vector update, or moderation reclassification.

## Regression boundaries

Maintained tests verify:

- the classification aside precedes and is distinct from the preserved body;
- the raw `auto-flagged` reason is not fused into prose;
- a matching flagged card is hidden while an unflagged match remains;
- Show/Hide state preserves the search and page;
- context replies preserve/filter the state;
- canonical ID, source ID, body text, raw event count, and canonical post count are unchanged by filter requests;
- explicit warnings remain expandable and NSFW remains excluded.

## Local observation

The shown New page had 100 cards and 20 flagged-spam asides. The hidden URL had 81 cards, no flagged asides, retained 80 canonical IDs from the first response, and omitted target `c78de8…c314`. That target's label was a separate aside before its body. This is a changing live feed, so the differing total is not a stable estimate of the flagged fraction.

Terra independently verified the visible Hide/Show states and followed a generated Replies link that retained `hide_spam=true`, rendered no flagged aside, and kept ordinary cards. Their initial capture crashed after loading both feeds but before saving their card arrays; they did not reload. Therefore their exact target-aside observation is limited to source/test/raw-body evidence, while the local HTTP evidence below preserves that rendered target check.

## Evidence

- `slop/verification/2026-09-14_spam-filter-tests.log`
- `slop/verification/2026-09-14_spam-filter-clippy.log`
- `slop/verification/2026-09-14_spam-filter-release.log`
- `slop/verification/2026-09-14_spam-filter-local-http.log`
- `slop/reviews/live-reader-review/local-alert-filter-review.md`
- `slop/reviews/live-reader-review/alert-filter-dom.json`

-- Pi/gpt-5.6-sol
