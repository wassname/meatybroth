# Parent relation versus app spam aside

Read-only local paired review on 2026-09-14:

```text
shown:  http://localhost:8088/?mode=new&before=1789376310
hidden: http://localhost:8088/?mode=new&before=1789376310&hide_spam=true
```

The raw-parent example is canonical event `d798efb94951cf8fe911fa4282a2f0f4b466d282e1cc853aff68aea9a5b37756`. In both states it renders the separate bold relation prefix `In reply to:` before the raw parent excerpt beginning `Alert: nostr:npub… has been identified as an AI-operated account …`. The alert words therefore remain clearly attributed to quoted Nostr text, not to the reader.

The app-filter example is canonical event `c78de8574286148d5dfc7f66f5235accab587cf94abf30663ce5d494caedc314`. In the shown state it has the distinct aside `Flagged spam: link farm · duplicate content` before the ordinary DiversZ body. The shown state has 100 unique IDs and 17 flagged-aside IDs. In the hidden state it has 84 unique IDs, the raw-parent example remains present with its relation prefix, target `c78…` is absent, zero cards have a `Flagged spam` aside, and `Show flagged spam` is visible.

This confirms the boundary: `hide_spam=true` filters cards with app auto-spam metadata; it does not filter raw quoted text that happens to say `Alert`. The full ordered card IDs, exact raw-parent and spam-target article DOM/HTML, flag IDs, state text, and navigation metrics are durable in [`parent-relation-filter-dom.json`](parent-relation-filter-dom.json), SHA-256 `16b70cd61b4313d4b226ac38dbf284317331852407d7f5340c8daabebb4ac7d8`.

Screenshots `50-local-parent-relation-shown.png` and `51-local-parent-relation-hidden.png` were opened locally and remain uncommitted for parent inspection. No provider, AWS, or public request was made.

-- Pi/gpt-5.6-terra
