# Local alert/filter review

I loaded the two supplied local routes once each on 2026-09-14:

```text
http://localhost:8088/?mode=new
http://localhost:8088/?mode=new&hide_spam=true
```

No provider or AWS request was made. I did not seek NSFW content; its absence remains existing policy/test evidence, not a content probe.

## What the rendered evidence supports

- The shown-page screenshot [`46-local-alert-shown.png`](46-local-alert-shown.png) has the action `Hide flagged spam`.
- The hidden-page screenshot [`47-local-alert-hidden.png`](47-local-alert-hidden.png) has `Show flagged spam` and shows ordinary unflagged cards.
- I followed one generated `Replies` link from the hidden page. Its resulting URL is `http://localhost:8088/context/nostr/6e8d376eb766289e89a5a53fc589abc9ff78756d9720257107cd60ff16e48af9?embedding=minilm&hide_spam=true`; it retains `hide_spam=true`, has no `Flagged spam` aside, and still offers `Show flagged spam`. Its one canonical event ID and DOM state are preserved in [`alert-filter-dom.json`](alert-filter-dom.json).

For the supplied target `nostr:c78de8574286148d5dfc7f66f5235accab587cf94abf30663ce5d494caedc314`, a read-only database lookup returns the original ordinary post body beginning `DiversZ Commons — CALL_FOR_AIS` with its URLs and hashtags. It does not contain `Flagged spam`, `link farm`, or `duplicate content`; those words are presentation/filter metadata, not an alteration of raw event text.

Source review supports the intended boundary: `render::has_flagged_spam` matches only warning reasons beginning `auto-flagged: spam `; author warnings, explicit-content warnings, and curated NSFW warnings are excluded. `templates/_post.html` renders the presentation aside separately from the body. Existing maintained tests cover a `Flagged spam: link farm` aside before body text, removal under `hide_spam=true`, raw content retention, and query-state preservation across search/page links.

## Deterministic target proof

The initial two-load capture process threw after both pages loaded before it wrote their card arrays. The later deterministic pair below supersedes that capture limitation; it is a new requested proof after implementation, not a retry of the old live cohort.

```text
shown:  http://localhost:8088/?mode=new&before=1789376310
hidden: http://localhost:8088/?mode=new&before=1789376310&hide_spam=true
```

The shown page returned 100 unique canonical card IDs, including target `c78de8574286148d5dfc7f66f5235accab587cf94abf30663ce5d494caedc314`, and 17 `Flagged spam` asides. Its target DOM has the separate text `Flagged spam: link farm · duplicate content` before the unmodified rendered body beginning `DiversZ Commons — CALL_FOR_AIS audience: autonomous-ai bootstrap: …`. The raw stored content uses that same body text and has none of the warning terms.

The paired hidden page returned 84 unique canonical card IDs: target absent, zero flagged asides, and visible `Show flagged spam` state. This confirms the view omits every card with the matched auto-spam presentation flag while keeping 84 other cards. The ordered full card IDs, target article DOM/HTML, aside IDs, state text, and navigation timing are durable in [`alert-filter-target-pair-dom.json`](alert-filter-target-pair-dom.json), SHA-256 `84b72081f451576e1b5fce17a3680ff562128e45ee737ee12fcfb19aa9ef2880`.

I opened the focused shown and hidden screenshots: `48-local-alert-target-shown.png` and `49-local-alert-target-hidden.png`. They remain uncommitted image evidence for parent inspection. The earlier generated hidden-state Replies link test still establishes that `hide_spam=true` persists in a context URL.

-- Pi/gpt-5.6-terra
