# meatybroth (Rust reader)

One Cargo executable serves the existing reader templates and CSS. It reads SQLite in-process; no Python runtime or SQL subprocess.

```sh
mkdir -p .local/live
: > .local/live/blocklist.txt
MEATYBROTH_DB="$PWD/.local/live/events.sqlite" cargo run --locked
```

Open http://localhost:8083. The default command creates a new SDK database, bootstraps signed Primal moderation lists, collects selected-relay text notes plus their profiles and referenced parents, and serves the same file. `MEATYBROTH_ADDR` overrides the bind address; `MEATYBROTH_RELAYS` is a comma-separated content-relay list; `MEATYBROTH_PROFILE_RELAYS` defaults to Purple Pages for root profile/follow hydration; `MEATYBROTH_ROOT` sets the Social root public key. Relay-cycle failures are recorded as gaps and do not stop HTTP. A collector-wide error stops collection with an explicit log while the reader remains available.

The repository selects nightly Cargo and enforces an eight-day minimum dependency publication age in `.cargo/config.toml`. `cargo update --dry-run` must report `as of 8 days ago`; build/test with `--locked`.

New, Relevance, Conversations and Social preserve the original ranking rules. Meaning, Similar, and keyword-labelled Topics use one exact local MiniLM vector space. Search URLs, 100-post pages, dates, expansion, thread context, profiles/self-declared NIP-05 addresses and status are implemented. Markdown uses pulldown-cmark and ammonia; post bodies cannot load images/scripts. The reader hides stored policy exclusions and Primal NSFW members, and preserves other warning labels.

Set `MEATYBROTH_READ_ONLY=1` only to inspect an existing projection without collection. Public collection keeps durable five-minute forward/backfill cursors and records interrupted or unreconciled intervals as gaps. It tries NIP-77 inventory reconciliation before advancing; selected public relays currently report it unsupported, so their EOSE-backed fallback remains explicitly incomplete, including same-second caps. Signed moderation snapshots refresh hourly before further admission; an invalid refresh stops collection but not HTTP. Set the explicit MiniLM space before startup:

```sh
MEATYBROTH_EMBED_BACKEND=minilm \
MEATYBROTH_EMBED_MODEL=sentence-transformers/all-MiniLM-L6-v2 \
MEATYBROTH_EMBED_DIMENSIONS=384 \
MEATYBROTH_EMBED_NORMALIZE=true \
MEATYBROTH_DB="$PWD/.local/live/events.sqlite" cargo run --locked
```

The resolved model revision plus model/tokenizer SHA-256 hashes define the space ID, so a changed snapshot cannot reuse or mix vectors. Local inference costs zero; the Bedrock backend remains disabled until paid-call approval.

```sh
cargo test --locked
```

The maintained tests exercise real HTTP handlers, including canonical follow metadata without `nostr_state`, nested Markdown images, excluded versus benign posts, cycles and expiry. The ignored matched-corpus test reads saved parity fixtures and does not run another language runtime.

Started by Pi/OpenAI; reader implementation by Pi/gpt-6-astra; embedding and reliability changes by Pi/gpt-5.6-sol. Architecture review: `/workspace/meatybroth/slop/reviews/2026-09-13_sdk-architecture-review.md`.
