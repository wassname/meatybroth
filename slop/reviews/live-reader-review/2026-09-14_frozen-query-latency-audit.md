# Frozen reader-query latency audit

- source: deployed `5b8ddd4371a582f35733c3ef611486989eb44dfd`
- database: `.local/deployment-handoff/events-8136.sqlite`
- database SHA-256: `cabe5372bd11729bd33468c311e9e980e5e20ddfa9bd47c973943be473cecd02`
- database bytes: 538,599,424
- audit copy: temporary; `5b8ddd4:src/posts.sql` applied only to the copy
- parameters: `now=max(events.created_at)=1789323794`, 30-day window, page 0, limit 101
- cohort: 13,788 eligible posts; 8,136 Titan vectors
- original output: `2026-09-14_benchmark-output-original.txt`
- corrected reproduction: `2026-09-14_benchmark-output-reproduction.txt`
- executable method: `benchmark_frozen_reader_queries.sql`

Reproduce from the repository root with SQLite only:

```bash
test "$(sha256sum .local/deployment-handoff/events-8136.sqlite | cut -d' ' -f1)" = \
  cabe5372bd11729bd33468c311e9e980e5e20ddfa9bd47c973943be473cecd02
cp --reflink=auto .local/deployment-handoff/events-8136.sqlite /tmp/latency-audit.sqlite
git show 5b8ddd4371a582f35733c3ef611486989eb44dfd:src/posts.sql | \
  sqlite3 /tmp/latency-audit.sqlite
sqlite3 /tmp/latency-audit.sqlite < \
  slop/reviews/live-reader-review/benchmark_frozen_reader_queries.sql
```

This was one bounded point measurement on a warm local filesystem. It is useful for decomposing query work, not for predicting EC2 wall time.

## Observations

### New ranking evaluates expensive eligibility twice

`queries.rs` defines every feed through an eligible view and, even without a search expression, materializes a second list before joining it back:

> `const ELIGIBLE: &str = "WITH eligible AS (`
>
> `let matches = if search.expression.is_some() { ... } else {`
>
> `"SELECT canonical_id, NULL AS bm25 FROM eligible WHERE :expression IS NULL"`
>
> `let prefix = format!("{ELIGIBLE}, matches AS MATERIALIZED ({matches})");`

— `/tmp/meatybroth-src-cab09b6/src/queries.rs:55-75`

The plan showed both sets of policy subqueries twice, an automatic covering index for the join, and a temporary B-tree for the final order. One query returned 101 rows in 0.268 s.

> `MATERIALIZE matches`
>
> `SEARCH event USING COVERING INDEX idx_events_created_at`
>
> `[policy/json correlated subqueries]`
>
> `SEARCH event USING INDEX idx_events_created_at`
>
> `[the same policy/json correlated subqueries]`
>
> `SEARCH x USING AUTOMATIC COVERING INDEX (canonical_id=?)`
>
> `USE TEMP B-TREE FOR LAST TERM OF ORDER BY`

### Card warning hydration scans the whole retained corpus twice

The feed calls `warning_map` for its 100 cards. `parent_excerpt_map` then calls `warning_map` again for parents:

> `let warning_map = render::warning_map(db, scored.iter().map(|(post, _)| post))?;`
>
> `let parent_excerpt_map = render::parent_excerpt_map(...)` 

— `/tmp/meatybroth-src-cab09b6/src/main.rs:489-493`

> `let parent_warnings = warning_map(db, parents.iter().copied())?;`

— `/tmp/meatybroth-src-cab09b6/src/render.rs:369-386`

Each call ignores its bounded input while loading NIP-36 tags and duplicate content:

> `FROM events event,json_each(event.tags) tag`
>
> `WHERE event.kind=1 AND json_extract(tag.value,'$[0]')='content-warning'`

> `FROM events`
>
> `WHERE kind=1 AND created_at>=unixepoch()-?1`
>
> `GROUP BY pubkey,content HAVING count(*)>1`

— `/tmp/meatybroth-src-cab09b6/src/render.rs:273-300`

For the New100 result (79 authors, 98 distinct author/content pairs, 31 parents):

| Query | Scope | Rows | Time |
|---|---:|---:|---:|
| NIP-36 warnings | all kind-1 events | 85 | 0.244 s |
| NIP-36 warnings | selected 100 event IDs | 1 | 0.0018 s |
| duplicate content | all 30-day kind-1 events | 397 | 0.225 s |
| duplicate content | selected 98 author/content pairs | 8 | 0.0305 s |
| profiles | all kind-0 events | 694 | 0.0100 s |
| profiles | selected 79 authors | 25 | 0.00068 s |

The two full warning scans cost about 0.47 s per call and can run twice per page. This accounts for about 0.94 s of local SQL time before card rendering. That closely composes with the separately observed ~1.145 s local full New response, but it is not an EC2 causal experiment.

### Similar reads every eligible vector

`embed::nearest` selects every eligible vector, decodes each 512-dimensional vector, computes each dot product, sorts, then truncates:

> `SELECT embedding.event_id,embedding.vector ...`
>
> `WHERE embedding.space_id=?1 AND event.created_at BETWEEN ?2 AND ?3`

> `let score: f32 = query.iter().zip(values)...sum();`
>
> `scored.sort_by(...)`
>
> `scored.truncate(limit);`

— `/tmp/meatybroth-src-cab09b6/src/embed.rs:1409-1447`

The SQL part returned 8,085 vectors and 16.82 MB in 0.336 s. Rust cosine calculation and sorting occur afterward. Similar is therefore exact $O(Nd)$ in eligible vectors and embedding dimensions. The 23.86 s production observation is not explained by card payload size alone.

### Reply counts are already bounded

The corrected selected-parent query used `reader_parent`, returned four rows for this New100 page, and took 0.0011 s. It is not a useful optimization target for this cohort.

The original ad hoc output reported zero rows/0.00023 s because it passed `posts.rowid` rather than `canonical_id`. The saved original output is preserved rather than rewritten. This error does not affect the plan or conclusion: both executions used the `reader_parent` index and were negligible relative to warning hydration.

## Inferences

1. **High confidence:** whole-corpus warning and duplicate scans are the largest avoidable New hydration work measured here. They run even though only 100 cards are rendered, and parent hydration can repeat them.
2. **Likely:** production CPU contention and corpus growth amplify these scans. The earlier production sample had a writer near one CPU core and substantially more events than this frozen DB. This is non-causal alignment, not proof.
3. **High confidence:** Similar remains linear exact search over all eligible vectors. Payload compression cannot reduce this ranking cost.
4. **Likely:** the no-expression `matches MATERIALIZED` self-join is avoidable work. Its plan evaluates the policy-heavy posts view twice and sorts to return 101 rows.

## Smallest changes

1. Restrict NIP-36 lookup to selected `source_id`s using `events.id IN (SELECT unhex(value) FROM json_each(:ids))`.
2. Restrict duplicate lookup to selected `(pubkey, content)` pairs. The measured indexed bounded form was 7.4× faster locally; the event-ID warning form was 134× faster.
3. Return immediately from `parent_excerpt_map` when no selected card has a parent. Otherwise hydrate warnings and identities for only the selected parents.
4. Special-case an absent search expression: select the bounded New page directly from eligible rows instead of materializing canonical IDs and joining eligibility back to itself.

Do not combine this with a concurrency change when evaluating reader latency. Similar exact-search replacement is a separate architectural decision; the bounded hydration changes are smaller and directly supported by the plans above.

— Pi/OpenAI
