# Low-risk maintained-source reduction

Baseline for this pass: `d8c658016542e96fdf15be17fc6a3f82c8d7342d`

## Changes

- Collection completion now has one transaction path. Reconciled runs pass no gap reason; unreconciled EOSE runs pass the existing reason. Run completion, optional gap insertion, and cursor advancement remain atomic.
- Direct-follow hydration now uses `Tags::public_keys()`. Parent hydration uses `Tags::event_ids()`.
- Fixed-k and DBSCAN clustering now call one loader for the same selected-space, 30-day `(event_id, vector, content)` rows. Their normalization checks, ordering, algorithms, and persistence remain separate.

## Tag parsing evidence

The installed `nostr 0.45.4` implementation at `/home/code/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/nostr-0.45.4/src/event/tag/list.rs:174-200` states:

> `public_keys()` first requires `t.kind() == "p"`, then requires tag content, then returns `PublicKey::from_hex(content).ok()`.
>
> `event_ids()` first requires `t.kind() == "e"`, then requires tag content, then returns `EventId::from_hex(content).ok()`.

Thus missing and malformed values remain omitted rather than becoming requested identities. I did not replace moderation-list parsing: that path additionally enforces canonical lowercase input, which the SDK accessor does not expose and should not silently broaden.

## Measured source change

Using the audit's raw-line rules, the two changed production files moved from 3,408 to 3,369 lines: **39 fewer maintained production lines**. The full current production count moved from 6,869 at `d8c6580` to 6,830: **39 fewer lines**. This is smaller than the audit's estimate and does not meet the original 3,416-line baseline; the remaining gap is 3,414 lines.

`git diff --numstat d8c6580` reports 51 added and 90 removed lines across `src/collect.rs` and `src/embed.rs`, net -39.

## Verification

The collection cursor/gap test, fixed-k embedding pipeline test, and DBSCAN membership/state test passed separately before the full suite. Full-suite, Clippy, and release-build logs are below.

- `slop/verification/2026-09-14_source-reduction-tests.log`
- `slop/verification/2026-09-14_source-reduction-clippy.log`
- `slop/verification/2026-09-14_source-reduction-release.log`

-- Pi/gpt-5.6-sol
