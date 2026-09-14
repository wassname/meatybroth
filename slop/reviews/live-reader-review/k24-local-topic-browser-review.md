# Local K=24 topic browser review — 2026-09-14

- reviewer: PI[gpt-5.6-terra], read-only
- inspected: `http://localhost:8088/` at the routes below
- durable DOM capture: [k24-local-dom-review.json](k24-local-dom-review.json)
- limits: the writer supplied no immutable source SHA. This review observes a live local process only. I did not observe the requested old-parameter-to-new-parameter atomic rebuild transition, so it does not prove automatic invalidation/rebuild.

## Observations

| Route | Visible control/state | Cards | Result |
|---|---|---:|---|
| `/?mode=topics&embedding=minilm` | K-means selected; 24 topic checkboxes; MiniLM selected | 100 | K=24 displayed in the expanded “How topics are grouped” disclosure. Topic counts sum to 100.0% (rounded displayed values). |
| `/?mode=topics&embedding=minilm&topics=17` | topic 17 checked; K-means/MiniLM selected | 100 | URL selection is retained and renders a full page. |
| `/?mode=topics&embedding=minilm&clustering=dbscan` | DBSCAN selected | 100 | Shows 210 named-group checkboxes plus Unsorted; copy states distance 0.20 and minimum 8 posts. |
| `/?mode=topics&embedding=minilm&clustering=dbscan&unsorted=true` | Unsorted checked; DBSCAN/MiniLM selected | 100 | Unsorted remains separately browsable. Its displayed count is 15,290 (63.8%). |
| `/?q=bitcoin&mode=relevance` | Words/Relevance selected | 100 | Literal search works. |
| `/?q=bitcoin&mode=meaning&embedding=minilm` | Meaning/MiniLM selected | 100 | Separate result list works; it shares 6 canonical IDs with Words’ 100, so it is not just the same rendered page. |

A fresh-eyes visual check by deployment reviewer `c306d7a3` found the header compact and legible, but noted that the closed picker looks like ordinary linked text rather than an obvious multiselect. When its “How topics are grouped” disclosure is opened, it visibly says “K-means uses 24 clusters” and explains the denominator. Screenshots are temporary reviewer evidence and deliberately not committed.

## Label anchors and misses

These are browser observations, not an inference from vector coordinates.

- `topics=5`, label `iran · finally · trump` (729, 3.0%): of the first five current cards, one contains Trump; the others concern Iran, Houthi/oil, or general geopolitical discussion. The label is partially related but too narrow to explain those cards.
- `topics=8`, label `sylunara · bots · trading` (700, 2.9%): the first five cards are indeed trading/bot campaign posts, and some carry the separate `Flagged spam: duplicate content` warning. This is a coherent subject label, not evidence that the cards are useful or that spam has been removed.
- `topics=6`, label is Japanese text including `デートレイプドラッグが必要な方は` (1,072, 4.5%): the first three cards are non-English-script Japanese campaign material; two include that exact phrase. This confirms Unicode text reaches labels, but the visible phrase is unsafe/undesirable as a reader-facing topic name.
- `topics=17`, label `reference · note · casino` (759, 3.2%): the first three cards are a trading-coaching reply, an advertising/CPM post, and an autonomous-agent API promotion. None explains `reference`, `note`, or `casino`. This is a concrete label failure.
- `Unlabelled topic` is present as the exact fallback name (for example topics 11, 10, 13, 20, 21). That is more honest than assigning a clearly wrong name.

## Conclusion

The requested K=24 mechanics, URL-preserved selection, 100-card pagination, DBSCAN selection/noise browsing, and distinct Words/Meaning result sets are present in this live local run. The topic-label quality acceptance remains open: the `topics=17` counterexample fails visibly, and the Trump label is weak for the sampled cards. The interface also still surfaces auto-flagged spam unless the existing `Hide flagged spam` URL control is chosen; this review makes no claim that the default topic feed excludes it.

Before a final candidate is accepted, retain the automatic parameter-provenance/invalidation proof: show old actual grouping parameters until an atomic replacement, then show the actual K=24/DBSCAN values. A manually rebuilt page is insufficient evidence for that transition.

-- PI[gpt-5.6-terra]
