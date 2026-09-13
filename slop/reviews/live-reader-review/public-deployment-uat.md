# Public cached-snapshot UAT

Read-only review of `https://meatybroth.com/` on 2026-09-13 18:35–18:40 UTC. I made ordinary GET requests only: no AWS/API calls, mutations or paid use. This is the intentionally partial cached Titan snapshot, not evidence that continuous collection or the MiniLM backlog is complete.

## Superseding final retest

Retested the same public URLs at 19:26–19:29 UTC after the Social fix.

- [`Social`](https://meatybroth.com/?mode=discovery&embedding=titan) now returns HTTP 200 with 100 cards in 3.32 s. The screenshot shows two-hop connection labels, replies, names/addresses where available and text-only links. This supersedes the earlier HTTP 500 blocker.
- Root, status, Titan Topics, [`Mixed #2`](https://meatybroth.com/?mode=topics&topic=2&embedding=titan), and cached [`Similar`](https://meatybroth.com/?mode=similar&similar=nostr:00000652698831e28fee4d85c314d6138529ff5e32fb78e3c978fdfb3ede7993&embedding=titan) all returned HTTP 200. Timings were 1.38–3.39 s.
- Cached-only scope remains explicit: the Meaning button is disabled, Vectors has only `Titan cached`, and the page states 119 PoW-biased cached posts with no AWS calls while browsing. No Meaning feature is claimed.
- The Feed dropdown still contains an enabled `Meaning` option. The earlier impossible-path issue remains; this retest does not convert it into a feature.
- The status snapshot is truthful: collection is paused, 12,510 recent posts are eligible, only 119 have Titan vectors, and 24/109 root follows have a stored contact list.

I opened all five final PNGs after capture:

- `27-public-social-fixed.png`: populated Social feed with explicit `2 hops · connection` values; no server error.
- `28-public-topic-final.png`: selected `Mixed #2` and the same heterogeneous 37-post cached membership.
- `29-public-similar-final.png`: clear source-context text and 100 cached results; visible semantic quality remains weak for the short source reply.
- `30-public-cached-controls-final.png`: disabled Meaning button, Titan-only selector, paused snapshot and 119/PoW/no-AWS disclosure.
- `31-public-status-final.png`: paused collector, old relay reports and incomplete contact-list coverage remain clear.

Final evidence: `public-final-retest-http.tsv`, saved `public-final-*.html`, and screenshots `27-*.png` through `31-*.png`.

## Earlier deployment result

The earlier public reader served New, Words, paging, Topics and cached-post Similar, but had the following issues.

### Earlier blockers

1. [`Social`](https://meatybroth.com/?mode=discovery&embedding=titan) returned HTTP 500 on four of four attempts, each in 1.03–1.05 s. The entire response was `Reader request failed; see server log.` Screenshot `24-public-social-error.png` shows a blank white error page.
2. Meaning is simultaneously disabled and offered. The dedicated button correctly says `Meaning unavailable`, but the Feed dropdown contains an enabled `Meaning` option. Selecting it with no text returns HTTP 400, `Meaning search requires text`; selecting it with the requested fixed query returns HTTP 400, `This Titan query was not cached during the approved one-off run; choose MiniLM`. The only Vectors option is `Titan cached`, so “choose MiniLM” is impossible on this deployment. Screenshot `25-public-meaning-error.png` shows the unusable selected state. `public-meaning-controls.json` records the enabled option and disabled button.

### Working outcomes

- Root and status returned HTTP 200. The root says collection is paused, recent coverage is incomplete, Titan has 119 cached posts, the cohort is PoW-biased, and browsing does not call AWS.
- Status says 12,510 eligible recent posts, 119 Titan posts embedded and 12,391 pending. Relay reports are dated 17:25 UTC while the page is a later snapshot; the paused wording makes that understandable. It also says 24 of 109 direct follows have a stored contact list and 85 remain missing/unavailable.
- [`Words: private key`](https://meatybroth.com/?q=private%20key&mode=relevance&embedding=titan) returned HTTP 200 and nine posts. The first four are repeated wallet/XRP promotions from one account and are visibly labelled duplicate-content spam. Later results include a Bitcoin merchant post. Words works, but this query is spam-dominated.
- [`New page 0`](https://meatybroth.com/?mode=new&page=0&embedding=titan) and [`page 1`](https://meatybroth.com/?mode=new&page=1&embedding=titan) each contained 100 unique events with zero overlap.
- [`Titan Topics`](https://meatybroth.com/?mode=topics&embedding=titan) rendered six links totalling 119 posts. [`Mixed #2`](https://meatybroth.com/?mode=topics&topic=2&embedding=titan) returned 37 members. The visible sample mixes XMR, tomatoes, local LLM advice, Nostr discussion and music; `Mixed` is honest.
- Cached [`Similar`](https://meatybroth.com/?mode=similar&similar=nostr:00000652698831e28fee4d85c314d6138529ff5e32fb78e3c978fdfb3ede7993&embedding=titan) returned HTTP 200 and 100 results for a Titan topic member. The source is the contextless reply “I'm glad it worked for you!” Top results discuss a good day, weekend reading, geopolitics, tomatoes and Formula 1. These are not recognizably similar in meaning; the weak source text and partial corpus are sufficient alternative explanations, so this does not isolate embedding quality.

## Fresh screenshot reading

I opened all eight public PNGs after capture.

- `19-public-root.png`: the normal text reader with disabled Meaning button and prominent paused/Titan warning. The first feed cards are ordinary text rather than telemetry, but include inflammatory news and missing-profile hex names.
- `20-public-words-private-key.png`: the query state is clear; repeated wallet promotions occupy the top cards and carry duplicate-content warnings.
- `21-public-page2.png`: a populated second page with text-only video/image links, replies, duplicate warnings and multilingual posts.
- `22-public-topic-clicked.png`: `Mixed #2` is visibly selected. Its first cards are heterogeneous, consistent with the label.
- `23-public-similar.png`: source-context wording and Titan score labels are clear; visible results have little semantic relation to the short source reply.
- `24-public-social-error.png`: only the raw server-error sentence on an otherwise blank page.
- `25-public-meaning-error.png`: Feed visibly says Meaning while the button says unavailable; the error tells the reader to choose an option that is absent.
- `26-public-status.png`: truthful paused snapshot, eligible count, contact-list coverage and old relay reports are readable. Detailed embedding accounting is below the viewport.

## Evidence

- `public-http-initial.tsv`, `public-http-routes.tsv`
- `public-page-check.txt`
- `public-meaning-controls.json`
- saved HTML `public-*.html`
- screenshots `19-public-root.png` through `26-public-status.png`

Remaining uncertainty: the earlier Social failure was stable across four requests but is superseded by the final HTTP 200 retest. I did not inspect server logs or external event/media targets. Semantic quality remains a partial-119-snapshot observation until the full Titan corpus is available.

-- Pi/gpt-5.6-sol
