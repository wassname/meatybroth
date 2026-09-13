# meatybroth (Rust reader)

One Cargo executable serves the existing reader templates and CSS. It reads SQLite in-process; no Python runtime or SQL subprocess.

```sh
MEATYBROTH_DB=/workspace/meatybroth/.local/sdk-parity/events.sqlite cargo run --locked
```

Open http://localhost:8083. `MEATYBROTH_ADDR` overrides the bind address; `MEATYBROTH_ROOT` sets the Social root public key.

The repository selects nightly Cargo and enforces an eight-day minimum dependency publication age in `.cargo/config.toml`. `cargo update --dry-run` must report `as of 8 days ago`; build/test with `--locked`.

New, Relevance, Conversations and Social preserve the original ranking rules. Search URLs, 50-post pages, dates, expansion, thread context, profiles/NIP-05 and status are implemented. Markdown uses pulldown-cmark and ammonia; post bodies cannot load images/scripts. The reader hides stored policy exclusions and Primal NSFW members, and preserves other warning labels.

This slice opens an existing SDK database read-only, including its current `posts`/FTS projection. Collection, schema ownership, cleanup, embeddings and topics are not integrated. Original runtime code has not been removed. This is not the completed application replacement.

```sh
cargo test --locked
```

The maintained tests exercise real HTTP handlers, including canonical follow metadata without `nostr_state`, nested Markdown images, excluded versus benign posts, cycles and expiry. The ignored matched-corpus test needs the reference Python environment only for verification; see `slop/verification/prepare_reader_parity.py`.

Started by Pi/OpenAI; reader implementation by Pi/gpt-6-astra. Architecture review: `/workspace/meatybroth/slop/reviews/2026-09-13_sdk-architecture-review.md`.
