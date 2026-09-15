# Reader UI fresh-eyes review — 2026-09-14

Scope: four supplied captures and the bounded DOM/state evidence only. No runtime or source inspection.

| Complaint | Result | Evidence |
|---|---|---|
| `With replies` must not look inert | pass for immediate feedback; latency not accepted | `control-state-and-loading.json` changes `aria-busy` from `false` to `true` and exposes `Loading…` immediately; final selected control is `conversations`. The final request still took 6.057 s in `after-routes-dom.log`, so this is not a latency result. |
| Flagged spam hidden by default | pass | After desktop (x=615–743, y=168) and narrow (x=187–328, y=320) say `Show flagged spam`, not `Hide flagged spam`. DOM logs the explicit opt-in text `Show flagged spam`; final With replies URL retains `hide_spam=true`. |
| Help is not a button | pass | After desktop x=751–785, y=167 and narrow x=329–358, y=320 show quiet `Help` text. DOM: `<a class="help" href="/about#reader-controls">Help</a>`. |
| Words/Meaning belong to Search | pass on desktop; narrow layout defect below | After desktop places `Search`, `Query`, `Method`, `Words` and `Search` together (x=582–1037, y=99–138). Before used separate `Words`/`Meaning` buttons (x=1023–1153, y=92–123). |
| Clear View/Search/Topics/Model prefixes | pass | DOM gives `<legend>View</legend>, <legend>Search</legend>, <legend>Topics</legend>, <legend>Model</legend>`. The after capture shows these headings; the earlier controls were a flat `View:`/`Group by:`/`Find:` sequence. |
| Semantic and keyboard-facing affordance | partial | The captures show radio buttons and selects, while the state evidence records checked values and no inline `onchange`. It does not record input element types or a Tab/keyboard exercise, so native keyboard operation is not established by the supplied DOM evidence. |
| Post text dominates; utility links recede | pass | Desktop after cards have dark author/title at x=200–350 and body text across x=200–900+ (for example `Posted this on IG…`, x=200–924, y=246–295); small utility links sit separately at x=963–1223, y=228. Narrow after retains body text as the large card region (x=13–362, y=421–575); utilities form a smaller row (x=80–362, y=399). |
| No label wraps away from its control | fail on narrow | In narrow after, `Method` is at x=288–338, y=187 while its `Words` select wraps to x=13–91, y=210; they are separated by a line and nearly the full viewport width. Desktop keeps them adjacent (x=827–973, y=109–138). |

## Smallest correction

1. No further correction from this bounded follow-up.

## Correction review — 2026-09-14

The sole narrow Method/Words failure is fixed. In `narrow-after-method-wrap.png`, `Method` (x=13–65, y=213–233) and its `Words` select (x=69–161, y=208–238) share one row. The recorded boxes confirm vertical overlap.

The bounded keyboard finding is also closed: the tab capture identifies the query as `INPUT type=search`, the feed as `INPUT type=radio`, the method/model/topic controls as `SELECT`, and Search as `BUTTON type=submit`. Keyboard actions produced one radio request with immediate `busy=true`/visible loading, and toggled the tested checkbox from false to true. Enter produced one request. Its immediate post-navigation snapshot is `busy=false`/loading hidden, but the MutationObserver captured `MEATY_BUSY true false`; that is evidence of the transient loading state, not a latency result or a general assistive-technology audit.

No review-evidence commit: the initial review was uncommitted, so this follows that convention.

## Cached With replies final-state review — 2026-09-15

No visible regression versus the accepted narrow corrected capture. `With replies` is selected; `Method`, its `Words` select, and Search remain together on one line; nothing visible overflows the 390px viewport. The page is in a finished state with cards rendered and no loading label. Card body text remains the largest content region, while author IDs, reply metadata and utility links remain smaller. There is no visible cache, refresh, SQL, or other engineering-progress copy. `Similar pending` remains a small post utility/status label rather than a cache message.

The cache checkpoint's warm/restart timings are not visual latency evidence, and this image cannot establish latency.

-- PI[gpt-5.6-terra]
