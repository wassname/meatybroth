# Topics latency P0

Runtime: persistent local MiniLM reader/writer on port 8088
Database snapshot: `.local/integrated/p0-frozen.sqlite`

## Diagnosis

The existing `topic_events` query starts from all membership rows in the selected space. On the observed MiniLM database this meant 22,173 memberships. SQLite then evaluated the policy-heavy `reader_post_events` view and built temporary B-trees for `DISTINCT` and final order before applying `LIMIT 101`.

The initial failure was real:

> `Slow reader mode=topics rows=100 rank_ms=14637 total_ms=17237`

A clean Rust build overlapped that request and amplified I/O contention. It did not create the underlying full-membership scan. Without a concurrent build, three old-binary Topics requests took 1.97–2.09 seconds; the old topic-event SQL alone took about 1.4 seconds.

The replacement starts from `events INDEXED BY idx_events_created_at`, joins membership and current reader eligibility, applies the same time, space, topic, policy, moderation, and retention predicates, preserves `created_at DESC,event.id`, and applies 101 only after those final conditions. `DISTINCT` is redundant because membership tables and `reader_post_events` each have at most one row per event/space.

Under the same active MiniLM plus release-build contention, paired SQL samples were:

| Query | Samples (s) |
|---|---|
| Membership-first | 1.43, 1.58, 1.32 |
| Event-first | 0.03, 0.02, 0.02 |

The latest fixed-topic rebuild remained epoch `1789355552`, about 19,500 seconds before the paired requests. Thus a concurrent topic rebuild did not explain them.

## Correctness gates

Fixed-now SQL parity passed for both cached spaces across fixed-k default/all/selected page 2, DBSCAN default/selected/Unsorted page 2, and empty results. All old and event-first queries returned identical ordered event IDs and SHA-256 hashes.

Separate old and new release binaries then read the same frozen database. The matrix covered those modes plus valid-topic empty page 100 and invalid direct context ID. Every case had identical status, ordered rendered event IDs, and full HTML after normalizing only `<time>` elements and request-time date-jump values. This includes 101-row SQL result pages used to produce 100 cards plus `has_next`.

After the replacement restarted as the active MiniLM writer, ten repeated fixed-k Topics requests all returned HTTP 200. Median total time was 0.383 seconds; maximum was 0.410 seconds. The process used about 77% CPU during the sample, collection remained active, and no Rust build ran concurrently.

The maintained SDK test now first proves that an actual reply appears in nearest-neighbor ranking, then proves the actual handler renders it exactly once before Similar replies and never as a recommendation.

## Evidence

- `slop/verification/2026-09-14_p0-topic-events-plan.log`
- `slop/verification/2026-09-14_p0-topic-events-new-plan.log`
- `slop/verification/2026-09-14_p0-topic-events-sql.log`
- `slop/verification/2026-09-14_p0-topic-events-event-first.log`
- `slop/verification/2026-09-14_p0-topic-paired-during-release-build.log`
- `slop/verification/2026-09-14_p0-fixed-now-sql-parity.log`
- `slop/verification/2026-09-14_p0-frozen-http-parity.log`
- `slop/verification/2026-09-14_p0-topic-duplicate-proof.log`
- `slop/verification/2026-09-14_p0-topics-after-active.log`
- `slop/verification/2026-09-14_p0-tests.log`
- `slop/verification/2026-09-14_p0-clippy.log`
- `slop/verification/2026-09-14_p0-topic-release.log`

-- Pi/gpt-5.6-sol
