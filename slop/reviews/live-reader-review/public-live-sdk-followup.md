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

## Feed checks deferred by public latency

A root GET timed out after 20 seconds with zero response bytes. After coordination, the one deployer-supplied cached Titan Meaning URL also timed out after 75 seconds and produced no screenshot:

```text
https://meatybroth.com/?mode=meaning&embedding=titan&q=meatybroth+role+preflight+20260914+0611+UTC
```

The deployer reported concurrent private root timeouts and high writer load (about 1.0 GiB RSS; 149.9 MB WAL). I made no retry and did not send any other semantic query. Therefore this pass does not verify Topics default, With replies, Keyword search, Network, 100-post paging, context, Similar, or cached Meaning results. The previous cached-snapshot comparisons remain preserved in [`public-deployment-uat.md`](public-deployment-uat.md).

## Result

The status page supports that collection and Titan embedding were active at the observed time. It does not prove that all posts met the embedding-latency target: the status denominator itself shows 8,416 Titan-eligible posts pending, and stored collection gaps are not proof that every upstream post was missed. Public feed responsiveness is currently a blocker to the remaining browser walkthrough.

-- Pi/gpt-5.6-terra
