# meatybroth (Rust reader)

One Cargo executable serves the existing reader templates and CSS. It reads SQLite in-process; no Python runtime or SQL subprocess.

```sh
mkdir -p .local/live
: > .local/live/blocklist.txt
MEATYBROTH_DB="$PWD/.local/live/events.sqlite" cargo run --locked
```

Open http://localhost:8083. The default command creates a new SDK database, bootstraps signed Primal moderation lists, collects selected-relay text notes plus their profiles and referenced parents, and serves the same file. `MEATYBROTH_ADDR` overrides the bind address; `MEATYBROTH_RELAYS` is a comma-separated relay list; `MEATYBROTH_ROOT` sets the Social root public key. Collection failure stops the server rather than silently serving stale data.

The repository selects nightly Cargo and enforces an eight-day minimum dependency publication age in `.cargo/config.toml`. `cargo update --dry-run` must report `as of 8 days ago`; build/test with `--locked`.

New, Relevance, Conversations and Social preserve the original ranking rules. Search URLs, 50-post pages, dates, expansion, thread context, profiles/NIP-05 and status are implemented. Markdown uses pulldown-cmark and ammonia; post bodies cannot load images/scripts. The reader hides stored policy exclusions and Primal NSFW members, and preserves other warning labels.

Set `MEATYBROTH_READ_ONLY=1` only to inspect an existing projection without collection. Public collection keeps durable five-minute forward/backfill cursors and records interrupted or unreconciled intervals as gaps. It tries NIP-77 inventory reconciliation before advancing; selected public relays currently report it unsupported, so their EOSE-backed fallback remains explicitly incomplete, including same-second caps. Signed moderation snapshots refresh hourly before further admission, and an invalid refresh stops collection. Embeddings and topics are not integrated, and the original runtime has not been removed pending replacement verification.

```sh
cargo test --locked
```

The maintained tests exercise real HTTP handlers, including canonical follow metadata without `nostr_state`, nested Markdown images, excluded versus benign posts, cycles and expiry. The ignored matched-corpus test needs the reference Python environment only for verification; see `slop/verification/prepare_reader_parity.py`.

Started by Pi/OpenAI; reader implementation by Pi/gpt-6-astra. Architecture review: `/workspace/meatybroth/slop/reviews/2026-09-13_sdk-architecture-review.md`.
