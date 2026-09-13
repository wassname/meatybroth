# sdk-collector (isolated prototype, M1 scaffold — NOT built, NOT wired)

Owner: SDK-migration worker. Only files under `collectors/sdk-collector/` belong here.
Do NOT edit `meatybroth/`, `tests/`, runtime units, or the live DB from this lane
(`ingest.py`/`store.py` stay with their owner until the Primal worker is done;
`ranking.py` stays with the ranking worker).

Status: HOLD per 2026-09-13 parent message — no app/schema/runtime/commit changes until
baseline parity screenshots are confirmed. This directory is new isolated scaffolding
only; it changes nothing existing and is currently UNBUILT (no `cargo` run yet).

Backend decision: `nostr-sqlite` file store (see `.local/reviews/nostr-sdk-reuse.md`
§7). ONE physical DB: SDK tables canonical; reader adds FTS5 + triggers + VIEWs in the
SAME file (an FTS index is not a second event DB). No exporter — deleted, never to be
committed. LMDB stays open as the more-new-code alternative (§7d), not a one-call swap.

Pre-store moderation: custom `AdmitPolicy::admit_event` (secret scan, blocklist, kind
gate) runs BEFORE save on live AND sync-download paths (report §7b) — no post-hoc purge.

Milestones: M1 scaffold (this) → M2 MockRelay acceptance (100cap / 520 same-second /
timeout-buffer / control fixtures + AdmitPolicy rejection-before-save proof) → M3
dry_run vs status relays → M4 same-file FTS/VIEW queries vs current reader output diff →
M5 cutover only on parent approval after baseline parity.
