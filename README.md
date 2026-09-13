# meatybroth — Rust replacement in progress

First slice: one Rust executable serves the existing reader templates/CSS and reads the SDK SQLite database in-process. No Python or SQLite subprocess. It opens the reference database read-only.

```sh
MEATYBROTH_DB=/workspace/meatybroth/.local/sdk-parity/events.sqlite cargo run --locked
```

Listens on `http://localhost:8083`. The existing comparison reader remains on port 8082.

Not yet parity: collection, conversation/social ranking, Markdown rendering, context/status routes, embeddings and clusters remain unimplemented. The visible page labels this limitation. Original templates/CSS are copied from the reference project, not rewritten.

Replacement work started by Pi/OpenAI. Do not adopt the earlier collector architecture wholesale; see `/workspace/meatybroth/slop/reviews/2026-09-13_sdk-architecture-review.md`.
