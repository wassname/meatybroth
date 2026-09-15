# meatybroth (Rust reader)

One Cargo executable serves the existing reader templates and CSS. It reads SQLite in-process; no Python runtime or SQL subprocess.

```sh
mkdir -p .local/live
: > .local/live/blocklist.txt
MEATYBROTH_DB="$PWD/.local/live/events.sqlite" cargo run --locked
```

Open http://localhost:8083. The default command creates a new SDK database, bootstraps signed Primal moderation lists, collects selected-relay text notes plus their profiles and referenced parents, and serves the same file. `MEATYBROTH_ADDR` overrides the bind address; `MEATYBROTH_RELAYS` is a comma-separated content-relay list; `MEATYBROTH_PROFILE_RELAYS` defaults to Purple Pages for root profile/follow hydration; `MEATYBROTH_ROOT` sets the Social root public key. Relay-cycle failures are recorded as gaps and do not stop HTTP. A collector-wide error stops collection with an explicit log while the reader remains available.

The repository selects nightly Cargo and enforces an eight-day minimum dependency publication age in `.cargo/config.toml`. `cargo update --dry-run` must report `as of 8 days ago`; build/test with `--locked`.

New, Relevance, Conversations and Social preserve the original ranking rules. Meaning, Similar, and keyword-labelled Topics use one selected vector cache. MiniLM and Titan have separate provenance and are never mixed. Search URLs, 100-post pages, dates, expansion, thread context, profiles/self-declared NIP-05 addresses and status are implemented. Markdown uses pulldown-cmark and ammonia; post bodies cannot load images/scripts. The reader hides stored policy exclusions and Primal NSFW members, and preserves other warning labels.

Set `MEATYBROTH_READ_ONLY=1` only to inspect an existing projection without collection. Public collection keeps durable five-minute forward/backfill cursors and records interrupted or unreconciled intervals as gaps. It tries NIP-77 inventory reconciliation before advancing; selected public relays currently report it unsupported, so their EOSE-backed fallback remains explicitly incomplete, including same-second caps. Signed moderation snapshots refresh hourly before further admission; an invalid refresh stops collection but not HTTP. Set the explicit MiniLM space before startup:

```sh
MEATYBROTH_EMBED_BACKEND=minilm \
MEATYBROTH_EMBED_MODEL=sentence-transformers/all-MiniLM-L6-v2 \
MEATYBROTH_EMBED_DIMENSIONS=384 \
MEATYBROTH_EMBED_NORMALIZE=true \
MEATYBROTH_DB="$PWD/.local/live/events.sqlite" cargo run --locked
```

The resolved model revision plus model/tokenizer SHA-256 hashes define the space ID, so a changed snapshot cannot reuse or mix vectors. Local inference costs zero.

A cached Titan deployment does not load MiniLM, invoke Bedrock, or require AWS credentials:

```sh
MEATYBROTH_READ_ONLY=1 \
MEATYBROTH_DEFAULT_EMBEDDING=titan \
MEATYBROTH_DB=/path/to/events.sqlite \
MEATYBROTH_ADDR=127.0.0.1:8088 \
./meatybroth
```

Do not set `MEATYBROTH_EMBED_BACKEND` in that long-running process. Titan Similar and Topics use cached vectors. Titan Meaning accepts only queries cached during the approved one-off run. `MEATYBROTH_RECLUSTER_ONLY=bedrock` rebuilds Titan topics offline without loading a model or calling AWS.

Topic labels rank Unicode terms by their per-document frequency inside the group versus the selected-space corpus. URLs are excluded, and the pinned MIT [stopwords-iso English list](https://github.com/stopwords-iso/stopwords-en/tree/ccc8898188850d8fb019d5f69c14a6635c3bd115) applies to English terms. Displayed terms must occur in two centroid-nearest representatives when two exist; otherwise the group is `Unlabelled topic`. Picker percentages use all eligible posts assigned in the selected embedding space and grouping method, including DBSCAN Unsorted posts.

<!-- Pi/gpt-5.6-sol -->

```sh
cargo test --locked
```

The maintained tests exercise real HTTP handlers, including canonical follow metadata without `nostr_state`, nested Markdown images, excluded versus benign posts, cycles and expiry. The ignored matched-corpus test reads saved parity fixtures and does not run another language runtime.

Started by Pi/OpenAI; reader implementation by Pi/gpt-6-astra; embedding and reliability changes by Pi/gpt-5.6-sol. Architecture review: `/workspace/meatybroth/slop/reviews/2026-09-13_sdk-architecture-review.md`.
