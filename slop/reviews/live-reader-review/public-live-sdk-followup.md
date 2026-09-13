# Public live-SDK follow-up

Read-only review of `https://meatybroth.com/` on 2026-09-13 22:26–22:31 UTC. This follows the public switch to continuous collection. I made ordinary public GETs only. The one Meaning request below used a deployer-supplied cached-query URL; it timed out, so it did not establish semantic retrieval or billing behavior.

## Status page

[`/status`](https://meatybroth.com/status) returned HTTP 200 in 6.18 s. I captured and opened [`32-public-status-live-sdk.png`](32-public-status-live-sdk.png).

The page at 22:27 UTC says:

- 17,602 eligible Nostr posts in the last 30 days.
- Live Rust SDK collection and reader share the SQLite file.
- Damus last report: 22:26 UTC. Primal last report: 22:27 UTC.
- 60 of 109 direct follows have a stored contact list.
- 986 unresolved collection gaps. The page says these are intervals without inventory proof, and counts do not establish complete relay coverage.
- Titan: 9,186 eligible embedded, 8,416 pending; newest eligible vector 22:27 UTC; 12,039,159 input tokens; $0.240783180 recorded and monthly cost; 73 uncertain and 5 reserved requests.
- MiniLM: 12,239 eligible embedded, 5,363 pending; newest eligible vector 18:24 UTC.

The visible header still renders `Semantic search unavailable` and `Titan cached`. The app owner says the status template is missing the `meaning_available` context while root checks actual queue availability. This review treats the header as a public UI mismatch, not proof that the Titan backend is unavailable.

## Recovery check: New100 and one cached Similar page

The original root GET timed out after 20 seconds with zero response bytes. The earlier one deployer-supplied cached Titan Meaning URL timed out after 75 seconds and produced no screenshot. The deployer then rebooted/repaired the host and reported `cb71d87` public. The parent independently loaded root before this follow-up: HTTP 200, TTFB/total 0.816 s, compressed wire body 4,754 bytes; it rendered Topics default and enabled Titan Semantic search. Root's zero-card topic index is expected, so a populated page is still required.

After that parent completion notice, I made exactly two serial browser navigations under the active writer:

| page | result | TTFB | total | transfer / encoded / decoded bytes | screenshot |
|:---|:---|---:|---:|:---|:---|
| `/?mode=new` | HTTP 200, **100 cards** | 8.513 s | 10.552 s | 3,393,293 / 3,392,993 / 3,392,993 | [`34-public-new100-live-sdk.png`](34-public-new100-live-sdk.png) |
| deployer-supplied cached Titan Similar URL | HTTP 200, **100 cards** | 13.027 s | 14.043 s | 213,841 / 213,541 / 213,541 | [`35-public-cached-titan-similar-live-sdk.png`](35-public-cached-titan-similar-live-sdk.png) |

These are browser navigation timings. The transfer/encoded/decoded figures are equal within header bytes, so this session did not observe compressed HTML on these two pages. This differs from the parent's root `curl` measurement: that response was a much smaller root/topic index and reported compressed wire bytes. A slow multi-megabyte New100 response is not the prior zero-byte hang.

New100's header says 10,012 cached Titan posts. All 100 newest cards were marked `similar pending`; that is an explicit, honest limitation, and no per-card semantic lookup was attempted. The full-page capture is too tall to read after ordinary thumbnailing, so I made and opened a local no-HTTP top crop, [`35-new100-top.png`](35-new100-top.png). It makes the populated Latest header/cards legible. It shows one visible card labelled `auto-flagged: spam duplicate-content` promoting a signup/referral/link campaign. This is a candidate moderation-policy/quality issue, not a confirmed policy violation: the label does not establish signed-list membership, and the agreed full-hide requirement is for NSFW while weak duplicate/spam warnings may intentionally remain visible pending review.

The crop also shows a card by `441d176ae740ef78` with `143508 more characters` hidden at age 20 s, and the same displayed author prefix again at age 33 s after intervening cards. This preserves a concrete performance/duplication observation without asserting that the full bodies or canonical event IDs are duplicates. The very large post is collapsed, consistent with the minimum-hidden-text display rule.

The cached Similar URL was supplied by the deployment worker and is expected not to invoke a provider or create a query embedding:

```text
https://meatybroth.com/?mode=similar&embedding=titan&similar=nostr%3A87ff20c4b1c6d408f12d6ebd0d6ed9923efa0f3edde92c84cff03f50274fd67c
```

It rendered the source/context explanation and 100 scored cards. However, the fresh screenshot shows its leading results are many consecutive URL-only Blossom video posts by the same author (Shawn Burden), with Titan cosine scores 0.874–0.847. This is a concrete poor-diversity/utility example, not evidence that Similar is useful. It should remain in the accepted-disappointments set for later retrieval/feed work.

No further Meaning request, retry, or paid semantic query was made. With replies, Keyword search, Network, context navigation, and a useful Meaning comparison were not exercised in this constrained pass. The previous cached-snapshot comparisons remain preserved in [`public-deployment-uat.md`](public-deployment-uat.md).

## Result

The status page supports that collection and Titan embedding were active at the observed time. New100 and cached Similar are now publicly responsive under active writing, but their 8.5–13.0 s TTFB is still slow. This review does not prove that all posts met the embedding-latency target: the status denominator itself shows 8,416 Titan-eligible posts pending, and stored collection gaps are not proof that every upstream post was missed. It also does not accept Similar quality: the observed duplicate-heavy, URL-only result prefix is an explicit counterexample.

-- Pi/gpt-5.6-terra
