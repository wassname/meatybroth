# Local Similar replies review

Observed 2026-09-14 from the one owner-supplied cached URL:

```text
http://localhost:8088/context/nostr/4e59eee5f2f214debabec91ac75fe6ee626a37d432eaa3c8aca1377292d8ef13?embedding=titan
```

The browser rendered Titan as the selected model. It returned 34 article IDs, all unique. The first 29 articles are the actual thread, followed by the exact heading `Similar replies:`, then five recommendations. The recommendation section therefore follows—not interleaves with—thread membership, and it does not repeat a root, ancestor, or thread/recommendation ID on this page.

The request has no semantic query text. It uses cached selected-space vectors and the context/Similar SQL route; it initiated no provider request. This does not establish that unrelated background work made no provider call.

## Retrieval quality: saved disappointment

This supplied source does **not** show a useful independent recommendation. Frozen-DB tag inspection corrects the earlier attribution: `710eef236e7d9c24…` is the displayed **author public-key prefix**, not an event ID. The supplied source event is `4e59eee5f2f214de…`, authored by that key. The five recommendation event IDs and authors are distinct:

| recommendation event | author | quoted-parent event |
|:---|:---|:---|
| `8ab990415ff2…` | `59fac445887c…` | `db76b936a3d4…` |
| `3e10427e578a…` | `a56e4b9f412d…` | `db76b936a3d4…` |
| `64d3f7ae41ea…` | `e58dd4f3c529…` | `db76b936a3d4…` |
| `35f78ee07622…` | `f09f3fe7c077…` | `db76b936a3d4…` |
| `2f33b23dedb7…` | `396b04ff4156…` | `db76b936a3d4…` |

All five actual authors differ, but every raw `e` tag points to the same canonical parent `db76b936a3d4be60…`, also authored by `710eef…`. They closely paraphrase cashless life in UAE cities: two repeat an Abu Dhabi wallet claim; another says nearly the same thing; the other two restate phone-pay convenience. `396b…` adds Al Ain/Apple Pay, but that is only a locality variation, not a meaningfully independent response.

A simple one-recommendation-per-quoted-parent selection would reduce these five recommendations to one without a new similarity threshold. It would not judge the topic or remove the recommendation; it would only avoid showing multiple replies to `db76…`. Which one survives should preserve the current ranking order (or be explicitly selected later), rather than adding a new quality score.

The source thread itself contains one more varied actual reply, `25324cd943605edc…`, which says tap-to-pay is useful but still keeps bills as a fallback. It is thread membership, not a recommendation, and is therefore not evidence that Similar replies found a useful contrast. I found no clear useful recommendation in this one supplied cached slice; this is negative evidence, not a claim about all cached Similar results.

Fresh screenshots were opened locally: `39-local-similar-replies-thread-top.png` and `40-local-similar-replies-recommendations.png`. They remain uncommitted image evidence for parent inspection.

-- Pi/gpt-5.6-terra
