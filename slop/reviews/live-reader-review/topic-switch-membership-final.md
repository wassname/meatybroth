# Final local topic-switch mechanics review

Observed 2026-09-14 on the owner-provided local release at `http://127.0.0.1:8088/` using `/tmp/meatybroth-dbscan-release.sqlite` read-only. No public HTTP, AWS action, or provider request was made.

## Identity

- source worktree at inspection: `/workspace/.worktrees/meatybroth-sla`, `git status --short` had no output at `ff0554d99ec84b74608cdb47ffc561ba678237bb`
- release database: `/tmp/meatybroth-dbscan-release.sqlite`, SHA-256 `811141c2c49457b3fc10e7828fd0c14559965ec8bbaf5922f451fb320406ae46`
- local port 8088 served the reviewed pages during the checks. The short-lived local runtime had exited by this report, so this note does not claim a later runtime PID or executable hash.

## Returned IDs, not card counts

For every request, I extracted the 100 rendered `<article id="nostr:…">` IDs and joined them against the selected space's membership table in the release database.

| request | returned IDs | membership condition | mismatches |
|:---|---:|:---|---:|
| `?mode=topics&embedding=titan&clustering=dbscan` | 100 | `post_dbscan_topics.topic_id != -1` in Titan space | 0 |
| `?mode=topics&embedding=titan&clustering=dbscan&unsorted=true` | 100 | `post_dbscan_topics.topic_id = -1` in Titan space | 0 |
| `?mode=topics&embedding=titan&clustering=kmeans&topics=1&topics=0` | 100 | `post_topics.topic_id IN (1, 0)` in Titan space | 0 |

This verifies the default DBSCAN group filter, explicit DBSCAN noise filter, and K-means multi-topic union for this frozen release database. It does not assess whether topics themselves are useful.

## Actual browser reset and MiniLM membership

The browser began at:

```text
http://127.0.0.1:8088/?mode=topics&embedding=titan&clustering=kmeans&topics=1&topics=0&page=2
```

Both topic boxes were selected. I changed only the visible `Model` select to `MiniLM`. The form's normal change handler navigated to:

```text
http://127.0.0.1:8088/?mode=topics&clustering=kmeans&q=&embedding=minilm
```

The final URL has no `topics`, `unsorted`, or `page` parameter. MiniLM was selected in the rendered control. Its 100 returned card IDs all join to `post_topics` in the MiniLM space; none was missing. This closes the prior local model-switch gap.

The browser action was a Topics GET with no Meaning query text. In the reviewed source, this route calls the selected-space SQL topic lookup; it does not call an embedding provider. This establishes that this browser operation initiated no provider call. It does not establish that no unrelated background writer call occurred.

-- Pi/gpt-5.6-terra
