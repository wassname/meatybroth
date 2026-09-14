# Local Similar replies review

Observed 2026-09-14 from the one owner-supplied cached URL:

```text
http://localhost:8088/context/nostr/4e59eee5f2f214debabec91ac75fe6ee626a37d432eaa3c8aca1377292d8ef13?embedding=titan
```

The browser rendered Titan as the selected model. It returned 34 article IDs, all unique. The first 29 articles are the actual thread, followed by the exact heading `Similar replies:`, then five recommendations. The recommendation section therefore follows—not interleaves with—thread membership, and it does not repeat a root, ancestor, or thread/recommendation ID on this page.

The request has no semantic query text. It uses cached selected-space vectors and the context/Similar SQL route; it initiated no provider request. This does not establish that unrelated background work made no provider call.

## Retrieval quality: saved disappointment

This supplied source does **not** show a useful independent recommendation. All five recommended replies quote the same source (`710eef236e7d9c24…`) and closely paraphrase cashless life in UAE cities: `59fac…` and `a56e…` repeat an Abu Dhabi wallet claim; `e58d…` says nearly the same thing; `f09f…` and `396b…` restate phone-pay convenience. `396b…` adds Al Ain/Apple Pay, but that is only a locality variation, not a meaningfully independent response.

The source thread itself contains one more varied actual reply, `25324cd943605edc…`, which says tap-to-pay is useful but still keeps bills as a fallback. It is thread membership, not a recommendation, and is therefore not evidence that Similar replies found a useful contrast. I found no clear useful recommendation in this one supplied cached slice; this is negative evidence, not a claim about all cached Similar results.

Fresh screenshots were opened locally: `39-local-similar-replies-thread-top.png` and `40-local-similar-replies-recommendations.png`. They remain uncommitted image evidence for parent inspection.

-- Pi/gpt-5.6-terra
