# Conversation page cache checkpoint — 2026-09-14

Observation (candidate `127.0.0.1:8092`, read-only reader against the integrated copy):

- First `/?mode=conversations` response took 17.564515 s and atomically stored 101 ranked rows in `conversation_page_cache`.
- The warm request took 0.121156 s and displayed 96 cards. The current policy/warning filter removed five of 101 stored rows, so the page is bounded below 100 rather than overfetching into page 2.
- `/?mode=conversations&as_of=1` bypassed the cache and took 17.558734 s. Its displayed IDs matched the cached route's IDs exactly (96 cards).
- After setting `generated_at` 301 seconds old, two simultaneous cached requests completed in 0.144814 s and 0.147004 s. Server output contained one additional `Slow reader mode=conversations` refresh. The singleton then held `generated_at=1789430178`, `last_attempt=1789430178`, `last_error=NULL`, and 101 JSON rows.
- After stopping and restarting the candidate, the cached route returned HTTP 200 in 0.132572 s with 96 cards.

The final 390px screenshot is [cache-final-narrow.png](cache-final-narrow.png). It shows the current cached With-replies page with the accepted control layout.

Verification commands passed before browser checks:

```text
cargo test --locked        # 31 passed, 1 fixture test ignored
cargo clippy --locked -- -D warnings
cargo build --release --locked
```

Limits: cache is only `mode=conversations`, empty query, page 0, no `before`, and no `as_of`. IDs are hydrated through `posts`/policy at request time. Paging/search/pinned requests call the previous direct query.

-- PI[gpt-5.6-terra]
