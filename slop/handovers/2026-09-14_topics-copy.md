# Topics and reader-copy handover

Status: implemented and tested locally on a copied frozen database. Not deployed. No AWS calls or public changes were made.

## What changed

- Topics now have two exact clustering views per embedding space: fixed-count K-means and DBSCAN.
- Repeated `topics` parameters select a union. With no selection, Topics shows all positive clusters. DBSCAN `unsorted=true` selects only DBSCAN noise; it can be combined with selected positive clusters.
- New vectors join the nearest current K-means centroid and an existing DBSCAN cluster only when they are within the configured cosine-distance threshold of a core point. New points do not become cores or merge clusters until the six-hour rebuild.
- Rebuild computation runs on a separate OS thread. It holds no SQLite write transaction while clustering or sorting representatives. Publication is one delete/insert transaction per clustering method, so readers see the previous complete result until commit. At commit it rechecks vector existence, current reader eligibility, and the 30-day window; events removed during computation cannot be reinserted into either topic method. Counts use only surviving membership, and a removed representative makes the label `mixed` rather than leaking stale text.
- Shutdown drains provider work as before, but does not wait for an unfinished topic rebuild. The maintenance thread is detached and process exit interrupts it; SQLite keeps the previous committed topics. A maintained test covers the non-waiting shutdown path.
- Topic IDs are reset in the browser when Model or Group by changes. Direct URLs with an ID absent from the selected space/method show a neutral error instead of silently treating it as another group. Cached MiniLM Topics and Similar posts remain selectable even when MiniLM inference is not active.
- Reader copy and card hierarchy were rewritten. Technical implementation detail moved into collapsed Status or grouping details. Similar remains a post action rather than a View option.

## Copied-database observations

Frozen source SHA-256 before any local schema cleanup:

> `cabe5372bd11729bd33468c311e9e980e5e20ddfa9bd47c973943be473cecd02`

The file contained 20,375 stored vectors across two incompatible spaces, not one 20,375-vector clustering input. Reader eligibility cleanup left 8,085 Titan vectors and 12,225 MiniLM vectors.

- Titan final release rebuild (8,085 vectors): 46.80s wall time, 224,548KB peak RSS.
- MiniLM final release rebuild (12,225 vectors): 98.54s wall time, 227,124KB peak RSS.
- Serial SQLite reads during the Titan rebuild completed in 0.00s.
- Serial HTTP reads during the Titan rebuild returned HTTP 200: K-means Topics TTFB 0.851s, DBSCAN TTFB 0.0045s, Status TTFB 0.0003s.
- Titan DBSCAN at cosine distance 0.20 and minimum 8 produced 63 positive clusters plus Unsorted: 1,292 grouped posts and 6,793 unsorted posts. 29 of 63 positive labels were honestly shown as `mixed` because representative posts did not support a stable three-term label.

These observations establish current-snapshot behavior, not month-scale capacity.

## Verification targets

- Full tests: `slop/verification/2026-09-14_goal3-final-tests-06.log`
- Clippy: `slop/verification/2026-09-14_goal3-final-clippy-06.log`
- Titan release profile: `slop/verification/2026-09-14_goal3-final-titan-profile.log`
- MiniLM release profile: `slop/verification/2026-09-14_goal3-final-minilm-profile.log`
- Concurrent reader checks: `slop/verification/2026-09-14_goal3-release-reader-during-rebuild.log`, `slop/verification/2026-09-14_goal3-release-http-during-rebuild.log`
- Stateful topic check: `slop/verification/2026-09-14_goal3-release-topic-state-check.log`
- Event-ID membership check: `slop/verification/2026-09-14_goal3-final-membership-check.log`
- Final local screenshot: `slop/verification/2026-09-14_goal3-topics-release-final.png`

## Deployment boundary

Do not deploy this commit without the parent supervisor's explicit deploy decision. The public 5b/cab trial, AWS credentials, systemd unit, and Docker container were not changed.

-- Pi/gpt-5.6-sol
