# New-feed self-join removal

Source baseline: `a55af98`
Database: `/tmp/meatybroth-dbscan-release.sqlite` (same copied frozen corpus before and after)
Query: `/?mode=new`, five serial release requests per version

## Change

When Words has no expression (`New` or empty Words search), the query now reads the bounded eligible page directly. It no longer materializes eligible canonical IDs and joins the policy-heavy eligible view to itself. Expression searches and the other ranking modes are unchanged.

The final query plan has one eligible event scan and one set of policy subqueries. It no longer contains `MATERIALIZE matches` or the second policy-heavy event scan.

## Observation

| Version | TTFB samples (s) | Median (s) |
|---|---|---:|
| Before | 0.315, 0.297, 0.289, 0.278, 0.319 | 0.297 |
| After | 0.051, 0.047, 0.051, 0.054, 0.059 | 0.051 |

The after median was 5.8× faster in this local point measurement. This does not predict EC2 latency.

The ordered 100-card ID SHA-256 remained:

> `820645b8e80b69369a4ba6ca54f81ad047b651c1e04722afe969889315ccd561`

Full rendered HTML was identical after normalizing the two request-time-dependent date-jump values.

## Evidence

- `slop/verification/2026-09-14_selfjoin-before.log`
- `slop/verification/2026-09-14_selfjoin-after.log`
- `slop/verification/2026-09-14_selfjoin-after-plan.log`
- `slop/verification/2026-09-14_selfjoin-tests.log`
- `slop/verification/2026-09-14_selfjoin-clippy.log`

-- Pi/gpt-5.6-sol
