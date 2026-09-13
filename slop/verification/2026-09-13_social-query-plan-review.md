# Social query-plan review

Date: 2026-09-13. Read-only review; no application or production database change.

## Observation

The app owner restored `http://localhost:8088/` read-only against `.local/rust-live-8086/events.sqlite`. A bounded Social request returned HTTP 200 in 15.34 s. The saved speed-4 request had timed out at 20 s while Root took 1.32 s, Meaning 1.56–2.19 s and Topic 1.97 s.

I used SQLite's online backup command to make `/tmp/rust-social-speed4.sqlite`. The 184,647,680-byte snapshot passed `PRAGMA integrity_check`, with 7,187 events, 7,187 reader rows, 141,093 materialized social edges, 238 stored contact-list authors and 109 direct root follows. It had no `sqlite_stat1` table.

The exact reach-2 Social SQL from `queries.rs`, including 100-card output and ordering, took:

- snapshot without statistics: 2.250 s;
- live database, first bounded CLI read: 7.665 s;
- live database, repeated read: 3.588 s;
- snapshot after `ANALYZE`: 0.231 s.

`ANALYZE` itself took 0.28 s on the snapshot.

## Exact plan change

Before statistics, the final eligible/matches join contains:

> `SCAN x`

where `x` is the materialized `matches` CTE, approximately the eligible corpus for an empty Social query. After `ANALYZE`, that same join contains:

> `BLOOM FILTER ON x (canonical_id=?)`
>
> `SEARCH x USING AUTOMATIC COVERING INDEX (canonical_id=?)`

This removes the repeated scan of materialized matches.

The graph side still materializes all 141,093 `social_edges`, scans that materialization for direct and endpoint edges, and creates temporary B-trees for both grouping stages. That is visible work, but it was not the dominant failure in this snapshot.

## Discriminator

I repeated the same query after analyzing one table at a time:

| snapshot statistics | query time↓ |
|---|---:|
| none | 2.250 s |
| `social_edges` only | 2.107 s |
| `reader_events` only | 1.942 s |
| `events` only | 0.252 s |
| full `ANALYZE` | 0.231 s |

A hypothetical reach-2 rewrite that avoided `edges AS MATERIALIZED` and removed the unreachable third-hop branch took 2.59 s without statistics, slower than the current 2.25 s. The evidence therefore supports event cardinality statistics before a graph-query rewrite.

## Diagnosis

Observed cause: `social.sql` creates and fills indexes but the database has no planner statistics. Without `events` statistics, SQLite declines to build an automatic index for the materialized matches join. `ANALYZE events` changes that decision and reduces the isolated rank query by about 9× on the same data.

The app owner then reproduced the change on the live database. Before statistics, the handler recorded `rank_ms=15644`, `replies_ms=1008`, `cards_ms=821`. `ANALYZE events` took 1.79 s; the next Social request returned HTTP 200 with 12 cards in 3.11 s and recorded `rank_ms=2019`, `replies_ms=193`, `cards_ms=867`. This confirms ranking—not card rendering—caused most of the timeout and that event statistics remove most of it. The remaining two-second live rank is slower than the 0.25-second snapshot and still includes filesystem/cache conditions not isolated here.

The app owner added event statistics to writable startup and retained the simpler materialized-edge design. Do not add a second reach cache unless a new query plan remains slow after statistics.

-- Pi/gpt-5.6-sol
