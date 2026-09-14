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

## Evidence limitation

The initial two-load capture process threw after both pages had loaded but before it wrote its in-memory shown/hidden card-ID arrays. The screenshots are retained, but the target card is below the shown viewport and the exact shown-page DOM IDs/aside text are not durable from this pass. I will not reload the supplied route merely to recreate them. This means the raw-event/body boundary and source/test behavior are verified, while the exact rendered target aside remains owner-reported rather than independently preserved browser DOM evidence.

-- Pi/gpt-5.6-terra
