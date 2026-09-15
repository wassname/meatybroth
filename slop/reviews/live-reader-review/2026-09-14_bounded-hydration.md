# Bounded card hydration

Source checkpoint: `ff0554d`
Database: `/tmp/meatybroth-dbscan-release.sqlite` (same copied frozen corpus before and after)
Query: `/?mode=new`, five serial release requests per version

## Change

- Profile lookups now scan only the selected card authors.
- NIP-36 and stored warning lookups now scan only selected event IDs.
- Duplicate detection starts from selected `(author, content)` pairs but counts matching events across the full retained corpus. A duplicate can remain off-page and still mark the displayed card.
- Parent hydration returns before any query when the selected cards have no parent. When parents exist, the same bounded identity and warning paths apply.

## Observation

| Version | TTFB samples (s) | Median (s) |
|---|---|---:|
| Before | 0.941, 0.817, 0.823, 0.866, 0.909 | 0.866 |
| After | 0.288, 0.287, 0.315, 0.284, 0.285 | 0.287 |

The after median was 3.0× faster in this local point measurement. This does not predict EC2 latency.

The ordered 100-card ID SHA-256 was identical before and after:

> `820645b8e80b69369a4ba6ca54f81ad047b651c1e04722afe969889315ccd561`

Full rendered HTML was identical after normalizing the two request-time-dependent date-jump values. A maintained regression also puts one duplicate outside page 0 and verifies that the displayed duplicate remains marked; it separately verifies a parent's NIP-36 warning hides the parent excerpt.

## Evidence

- `slop/verification/2026-09-14_hydration-before.log`
- `slop/verification/2026-09-14_hydration-after.log`
- `slop/verification/2026-09-14_hydration-tests.log`
- `slop/verification/2026-09-14_hydration-clippy.log`

-- Pi/gpt-5.6-sol
