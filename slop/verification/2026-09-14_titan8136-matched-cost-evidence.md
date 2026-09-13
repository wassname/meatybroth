# Frozen Titan matched-cost evidence

Observed 2026-09-14. Read-only SQLite CLI queries against `.local/deployment-handoff/events-8136.sqlite`, SHA-256 `cabe5372bd11729bd33468c311e9e980e5e20ddfa9bd47c973943be473cecd02`. No live/production database, model call, or retry was used. The reader's source predicate is `posts` plus `created_at BETWEEN fixed_snapshot_time - 30 days AND fixed_snapshot_time`; `post_embeddings` records aggregate completion for each space.

```sql
WITH eligible AS (
  SELECT source_id FROM posts
  WHERE created_at BETWEEN 1786743846 AND 1789335846
), spaces AS (SELECT id, backend FROM embedding_spaces),
titan AS (
  SELECT r.event_id, SUM(r.actual_tokens) titan_tokens,
         SUM(r.actual_nusd) titan_nusd, COUNT(*) requests
  FROM embedding_requests r JOIN spaces s ON s.id=r.space_id
  WHERE s.backend='bedrock' AND r.status='succeeded'
  GROUP BY r.event_id
), mini AS (
  SELECT pe.event_id, pe.input_tokens minilm_tokens
  FROM post_embeddings pe JOIN spaces s ON s.id=pe.space_id
  WHERE s.backend='minilm'
)
SELECT CASE
  WHEN EXISTS(SELECT 1 FROM eligible e WHERE e.source_id=lower(hex(t.event_id)))
   AND m.minilm_tokens IS NOT NULL THEN 'matched_completed_eligible'
  WHEN EXISTS(SELECT 1 FROM eligible e WHERE e.source_id=lower(hex(t.event_id)))
   THEN 'eligible_titan_no_minilm_match'
  ELSE 'not_currently_eligible'
 END cohort,
 COUNT(*) events, SUM(requests) requests, SUM(titan_tokens) titan_tokens,
 SUM(titan_nusd) titan_nusd, SUM(m.minilm_tokens) minilm_tokens
FROM titan t LEFT JOIN mini m ON m.event_id=t.event_id
GROUP BY cohort;
```

| cohort | events | requests | Titan tokens | actual cost | MiniLM tokens |
|:---|---:|---:|---:|---:|---:|
| matched completed eligible | 7,537 | 7,667 | 1,169,047 | $0.02338094 | 1,129,414 |
| eligible Titan; no MiniLM match | 599 | 1,351 | 2,445,982 | $0.04891964 | — |

The matched cohort has 155.107735 Titan tokens/event and Titan/MiniLM ratio 1.035092. At $0.02/million input tokens, multiplying 155.107735 by the illustrative 918,000-note monthly volume gives $2.847778. The all-8,136 charged-event ledger rate is $0.07230058 / 8,136 × 918,000 = $8.157809. This is not a production forecast: its low-four-hex-zero rate is 930/7,537 (12.3%), versus 1,010/13,874 (7.3%) for all reader-eligible posts; it also excludes the 599 long/no-MiniLM-match events.

```sql
SELECT r.status, COUNT(*) request_rows, COUNT(DISTINCT hex(r.event_id)) events,
       SUM(r.actual_tokens) actual_tokens, SUM(r.actual_nusd) actual_nusd,
       SUM(r.reserved_nusd) reserved_nusd
FROM embedding_requests r JOIN embedding_spaces s ON s.id=r.space_id
WHERE s.backend='bedrock'
GROUP BY r.status;
```

| status | rows | events | actual tokens | actual cost | reserved budget |
|:---|---:|---:|---:|---:|---:|
| succeeded | 9,018 | 8,136 | 3,615,029 | $0.07230058 | $1.42255108 |
| uncertain | 1 | 1 | 3 | $0.00000006 | $0.00000004 |
| reserved/held | 3 | 3 | — | — | $0.00049152 |

The exact fixed-time predicate returns 13,874 posts and 8,136 completed Titan aggregate vectors. The dated handoff log instead reports 13,852 eligible / 8,124 vectors; this discrepancy is retained, not reconciled by assertion. Quick negative checks on the same frozen DB found 63 valid JSON values (50 Titan), 25 JSON objects (12 Titan), and 3 trim-blank values (0 Titan); none alone explains the 22-post/12-vector discrepancy. For the dated first 119 succeeded request rows (`requested_at <= 1789306718`), the same frozen ledger returns 119 events, 7,247 actual tokens, and $0.00014494, reproducing the preserved 119-call evidence. The snapshot is application-ledger evidence, not an AWS invoice.

-- Pi/gpt-5.6-terra
