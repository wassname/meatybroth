# Integrated local MiniLM proof plan

Question: Does the current Rust writer collect a new eligible event, embed it locally with MiniLM, assign it to the existing between-rebuild topic state, answer a nonliteral Meaning query, remain responsive, and reuse vectors after restart?

The source database is the frozen local copy; `.local/integrated/events.sqlite` is the persistent owned writer copy. The public reader and AWS are out of scope. The existing read-only demo stays on port 8088 until replacement.

## Options

| Option | Evidence | Decision |
|---|---|---|
| Read-only copied corpus | Reader and old vectors only; cannot prove collection or embedding | Reject |
| Embed-only run | Local inference only; cannot prove the collector-to-embedding handoff | Reject |
| Integrated writer on temporary port 8089 | Same process owns collection, local inference, topic assignment, and HTTP | Use, then move the proven writer to 8088 |

## Predictions

| Risk | Expected success | Distinguishing failure |
|---|---|---|
| Model provenance | Existing MiniLM space resolves and old vectors remain usable | New incompatible space or startup mismatch |
| Relay collection | A post absent from the baseline becomes eligible | Event count moves but no eligible post, or no arrivals |
| Between-rebuild assignment | The new vector receives fixed-k and DBSCAN membership while topic `created_at` remains the baseline value | Topic rebuild timestamp moves, or the vector remains unassigned |
| Meaning | A nonliteral query returns semantically relevant cached posts and does not require Titan/AWS | Empty/literal-only results or provider substitution |
| Responsiveness | Repeated ordinary New requests succeed while collection/embedding runs | Timeout or high blocking coincident with the embedding log |
| Restart reuse | Vector count and new event vector persist; restart produces no replacement embeddings before serving | Vector count resets, space changes, or old post is embedded again |

The nearest-search null is lexical overlap: a Meaning result dominated by exact query terms does not prove semantic retrieval. The check will use a query whose useful returned text does not contain the query words.

-- Pi/gpt-5.6-sol
