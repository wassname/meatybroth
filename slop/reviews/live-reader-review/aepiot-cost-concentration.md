# aePiot payload and cost review

Supervisor observation, 2026-09-14 UTC — Pi/OpenAI.

Source: frozen `.local/deployment-handoff/events-8136.sqlite` (verified handoff SHA `cabe5372bd11729bd33468c311e9e980e5e20ddfa9bd47c973943be473cecd02`). No live database or model calls.

## Observations

Author public key: `441d176ae740ef78b4b22129da2aea29aa2caf20dbf53bb8463ddd4fea90cf47`.

- 86 stored posts, **86 distinct exact bodies**; exact-input caching alone would not eliminate these.
- Every body contains aePiot/allgraph/headlines-world campaign domains.
- URL-only lines constitute 91.87–97.62% of body characters (median93.40%). The other sampled lines are unrelated hashtag/title fragments, not article paragraphs. This is character share, not a token measurement.
- 51 of these posts account for 917 successful Titan requests, 2,749,271 returned input tokens and $0.054985420. The whole frozen run charged3,615,029 tokens/$0.072300580 across8,136 events: this author contributes about76% of tokens/cost from about0.63% of charged events.
- Public screenshot `35-new100-top.png` independently shows the same author prefix with143,508 hidden characters and another post13s apart. That screenshot alone did not establish body duplication; frozen inspection now establishes that bodies differ.

Paid-request query:

```sql
SELECT count(*), count(DISTINCT r.event_id),
       sum(r.actual_tokens), sum(r.actual_nusd)
FROM embedding_requests r
JOIN events e ON e.id=r.event_id
JOIN embedding_spaces s ON s.id=r.space_id
WHERE s.backend='bedrock' AND r.status='succeeded'
  AND e.pubkey=X'441D176AE740EF78B4B22129DA2AEA29AA2CAF20DBF53BB8463DDD4FEA90CF47';
```

Observed output: `917|51|2749271|54985420`.

Payload counts and bounded per-post samples: [`../../verification/2026-09-14_aepiot-payload-shape.json`](../../verification/2026-09-14_aepiot-payload-shape.json). Temporary read-only analysis source is `/workspace/meatybroth/.local/reviews/aepiot-inspect.cjs`.

## Direct examples

Event `74a6dafc5d062071bd55298e4b55d86d485b1dbec4e5812f56d157d97c125910` has152,536 characters. Its opening title lines include:

> #SIGNED #SEALED #DELIVERED TV #SERIES
> #ANTIQUIZATION
> #ALEXANDER W #WEDDELL
> #STEVEN #STILIANOS

Those lines alternate with search links. One link's query text says:

> AEPIOT:+INDEPENDENT+WEB+4.0+SEMANTIC+INFRASTRUCTURE+EST.+2009.+HIGH-DENSITY+FUNCTIONAL+SEMANTIC+CONNECTIVITY+WITH+100/100+TRUST+SCORE+AND+KASPERSKY+INTEGRITY+VERIFIED

This is the poster's promotional assertion, not verified trust/security evidence. Other inspected bodies similarly mix unrelated subjects and campaign/search links; none of those links was followed or its instructions executed.

## Interpretation and proposed decision

This is a strong candidate for specifically reviewed search-promotion spam exclusion, not a reason to ban long posts, URLs, one-word greetings or socially distant authors. Cost concentration is a reason to investigate, **not by itself a moderation justification**. The repeated promotional structure across86 different bodies supplies the relevant content evidence.

Request an independent review before applying a narrow campaign/author exclusion under the user's existing reviewed-spam authorization. Preserve all charged-request history and distinguish historical spend from prospective savings. No exclusion has been applied by this review.

## Independent frozen-data assessment — Pi/gpt-5.6-terra

I independently inspected only the same frozen database; I did not follow a URL or execute text from any post.

**Observations.** The exact public key has 86 kind-1 events, no other stored event kind, and zero events with an `e` reply tag. Their timestamps cover 2,497 seconds (about 42 minutes). A fresh SQL negative check found zero of 86 without *all three* campaign strings `aepiot`, `allgraph`, and `headlines-world`; the shortest is 60,251 characters. Three bounded body openings/endings show a repeated alternation of unrelated terms and campaign/search links; one ends in a generated instruction asking a third-party service to analyze terms. I did not treat that instruction as an instruction to follow.

**Counterexample search.** There are no non-author kind-1 bodies at least 60,000 characters in this frozen database. At a lower 10,000-character threshold, the only non-author candidate with at least ten URL occurrences is another account's 33,577-character post using the same aePiot search-link pattern. This does not prove that legitimate long directory posts do not exist elsewhere; it means this frozen corpus supplied no benign counterexample to the observed structure. The author’s 86 bodies are distinct, so exact-input caching would not have avoided their spend. Nor would a rule about length, URL share, multilingual text, arbitrary hashtags, greetings, low social connectivity, or cost be justified by this inspection.

**Decision.** A *prospective*, reviewed condition matching this exact public key **and** at least one of the three campaign domains/terms is justified for the observed corpus. The content pattern—not cost concentration—is the reason: repeated mass publication of unrelated labels paired with promotional/search links, without article prose or conversation. Requiring both the key and campaign marker limits collateral exclusion if the account later posts unrelated text. Do not generalize this to all long or URL-heavy posts, all users of the domains, or other authors without another review; the one other matching account is evidence to inspect, not authority for an automatic campaign-wide block. Do not delete source events or rewrite historical ledger costs. No exclusion has been applied.
