# Ranked-ID conversation cache checkpoint — 2026-09-15

Observation: candidate `127.0.0.1:8092` used the final release binary against `/workspace/.worktrees/meatybroth-sla/.local/integrated/events.sqlite`. It did not change production `8088`.

## Stored payload

- Before the rank-record change, the singleton `rows_json` was 88,029 bytes for 101 serialized `Post` objects.
- After a fresh cold fill, it was 17,560 bytes for 101 records (80.1% smaller).
- SQLite JSON inspection found only these keys, repeated for each record:

  ```text
  canonical_id,n_reply_authors,n_replies,latest_activity,root_present
  ```

  It contained no text, profile, source ID, URL, or parent fields.

## Browser-level HTTP measurements

| Route | Seconds | Cards | Observation |
|---|---:|---:|---|
| `/?mode=conversations` cold | 18.125 | 94 | ranked 101 and atomically stored cache |
| same warm | 0.168 | 94 | hydrated current eligible posts from cache |
| restart, same warm route | 0.143 | 94 | persisted singleton reused |

Cold and warm pages displayed the same 94 IDs in the same order. `conversation_page_cache_keeps_order_and_hydrates_policy` compares the stored rows to the direct `queries::feed` order. The 101-to-94 difference is current warning filtering after ranked IDs are hydrated; no overfetch into page 2 was added.

After making the cache stale, two concurrent cached requests returned in 0.134 and 0.143 seconds. The candidate log contained three ranking entries from the cache verification: its cold fill, a repeated direct-ranking observation from the superseded `as_of` experiment, and one cache refresh. The `as_of` route was removed because it did not implement a snapshot. The post-refresh row was `generated_at=1789431039`, `last_attempt=1789431039`, `last_error=NULL`, with 101 / 17,560-byte rank rows.

## Maintained test and build

`conversation_page_cache_keeps_order_and_hydrates_policy` compares direct `queries::feed` order with the stored rank rows, then verifies the five-field serialized record, error-update retention of the old generation/ranks, exposure of `last_error`, and current policy exclusion during hydration.

```text
cargo test --locked        # 31 passed; 1 fixture test ignored
cargo clippy --locked -- -D warnings
cargo build --release --locked
```

No deployment was performed.

-- PI[gpt-5.6-terra]
