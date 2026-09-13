# Collection-enabled, no-embedding deployment audit

Target: isolated container run of exact revision `18b4f629e1ad4b46c1261abf9167c3bcf8648f2f`; source DB was a consistent SQLite backup, not the active Titan DB. Image `meatybroth-rust:18b4f62`; binary SHA-256 `8a4ceca3071c42fd0712efb381304119fa4b6ed92312feee592ccfc1246f07cf`.

| stage | expected | observed | expected? | clues | missing metric | consequence |
|---|---|---|---|---|---|---|
| isolated startup | writable copy, reader starts | reader started; schemas completed | yes | startup log below | none | reader path is testable |
| zero-paid-call guard | no embedding/AWS initialization | `embedding_setup_ms=0`; no backend, budget or AWS env | yes | env inspection below | packet-level trace | strong configuration evidence, not network proof |
| SDK collection | connect to relays and ingest | tokio worker panicked in rustls before collection | no | exact panic below | relay connection counts | collection feasibility not established |
| new-post path | upstream post → canonical event → HTTP | event count/latest unchanged | no | before/after counts below | a new event ID | core acceptance evidence absent |
| moderation/retention | excluded input stays out; retained event remains | not reached for new inputs | unclear | collector never connected | new positive/negative fixtures | no collection-policy conclusion |
| HTTP availability | status/Social/Topic responsive | all HTTP 200 after collector panic | yes | timings below | cold/repeat distribution | reader degrades independently as intended |
| resources | bounded CPU/RAM | 97.06 MiB of 1 GiB; 0% sampled CPU; 3 PIDs | yes | `docker stats` | peak during real collection | idle/panic state only |
| persistence | isolated DB contains new canonical event | counts unchanged | no | exact counts below | none | resolve condition not met |

## Chronological evidence

The isolated database passed `pragma integrity_check` before launch and contained 18,053 canonical events, 13,874 reader posts and 15,986 vectors. Its latest event was `2026-09-13 18:23:14` UTC. Container configuration set `MEATYBROTH_READ_ONLY=0` and `MEATYBROTH_DEFAULT_EMBEDDING=titan`; inspection found no embedding-backend, Bedrock-budget, AWS profile, region, access-key, secret-key or session-token variable. The root filesystem was read-only, the isolated `/data` copy was writable, capabilities were dropped, no-new-privileges was set, and limits were 1 GiB and 1.5 CPUs.

Primary runtime log, complete retained stderr at `/tmp/pi-processes-raT92q/proc_ac95-stderr.log`:

> Startup stage sdk_open_ms=3
> Startup stage posts_schema_ms=19
> Startup stage coverage_schema_ms=9
> Startup stage embedding_schema_ms=0
> Startup stage social_schema_ms=44
> Startup stage derived_cleanup_ms=1723
> Startup stage analyze_events_ms=59
> Startup stage embedding_setup_ms=0
> Rust reader listening on http://0.0.0.0:8088

This establishes that local embedding/API setup did no work before the reader listened. It does not establish absence of every possible network syscall; the stronger evidence is that no backend or credential source was provided.

The first attempt stopped collection explicitly because the runtime contract requires `/data/blocklist.txt` beside the database:

> Collector stopped with an explicit error: No such file or directory (os error 2); HTTP reader remains available

Adding an empty isolated blocklist let startup reach SDK networking, which immediately panicked:

> thread 'tokio-rt-worker' (7) panicked at /home/code/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rustls-0.23.43/src/crypto/mod.rs:249:14:
>
> Could not automatically determine the process-level CryptoProvider from Rustls crate features.
> Call CryptoProvider::install_default() before this point to select a provider manually, or make sure exactly one of the 'aws-lc-rs' and 'ring' features is enabled.

This is a dependency-initialization failure before evidence of relay connection or event ingestion. The container main process stayed alive because the reader and collector are separate tasks.

After the panic, complete HTTP requests returned:

> `/status http=200 bytes=24547 time=0.860851`
>
> `/?mode=discovery&embedding=titan http=200 bytes=248587 time=3.410926`
>
> `/?mode=topic&embedding=titan http=200 bytes=3235609 time=1.898427`

The final database read remained exactly 18,053 events, latest `2026-09-13 18:23:14`, 13,874 reader posts and 15,986 vectors. No new-post sample existed to inspect because collection never began. The container was then deliberately stopped; process exit137 is teardown, not the collector failure.

## Hypotheses

### H1 [bug | Almost Certain | 98%]

- **Mechanism:** both rustls crypto features are present and no process default was installed before Nostr SDK TLS initialization.
- **Evidence:** “Could not automatically determine the process-level CryptoProvider … Call CryptoProvider::install_default()”. This is the library's direct panic at the first networking stage.
- **Contrary evidence:** HTTP startup works, but it need not initialize this TLS path.
- **Discriminating test:** install exactly one explicit provider before SDK networking; relay connection logs and a new canonical event should replace the panic.
- **Fix/action:** app owner should install the chosen rustls provider once at process startup, then repeat this same isolated run.
- **Interpretability:** partial; reader/no-paid-backend behavior is interpretable, collection is not.

### H2 [harness | Almost Certain | 95%]

- **Mechanism:** writable collection requires a blocklist file even when policy has no entries.
- **Evidence:** without it, the exact log was “No such file or directory”; with an empty file, execution advanced to the rustls panic.
- **Contrary evidence:** the message did not name the path; source code reads `blocklist_path` immediately before relay work.
- **Discriminating test:** remove only the file and observe the same ENOENT; already reproduced once.
- **Fix/action:** production collector config must include `/data/blocklist.txt`, with documented ownership and update semantics.
- **Interpretability:** yes for deployment configuration; no for actual policy behavior.

### H3 [measurement | Highly Likely | 80%]

- **Mechanism:** environment inspection strongly guards paid calls but does not observe packets.
- **Evidence:** `embedding_setup_ms=0` and all backend/AWS/budget variables were absent.
- **Contrary evidence:** a hidden instance-role or default provider could exist in another environment; this local container had no such mount or credential env.
- **Discriminating test:** repeat with network accounting or an egress-deny rule permitting only configured relays, and observe zero AWS endpoints.
- **Fix/action:** retain explicit absent-backend startup logging and add an automated assertion that collection mode does not construct an embedder when backend is absent.
- **Interpretability:** yes for this isolated container's configuration; partial for generalized deployment safety.

## Decision

1. **Resolve-condition verdict:** not met. The requested proof was “new upstream post → canonical → HTTP”; the collector panicked before relay collection and event counts stayed unchanged.
2. **Prediction check:** zero embedding initialization was supported; reader responsiveness after collector failure was supported; recent collection, retention and moderation predictions remain unresolved.
3. **Earliest unsupported link:** relay connection. A successful connection followed by one newly observed source event would support it.
4. **Validity:** `P(result is invalid for deciding collection feasibility) ≈ 0.90–0.98`; the result is a credible negative for revision `18b4f62` as a collection-enabled deployment, while its reader and zero-paid-backend observations remain credible.
5. **Highest-information clues:** (1) direct rustls provider panic; (2) unchanged latest event/count; (3) `embedding_setup_ms=0` plus absent credential/backend env.
6. **Missing metrics:** relay connection result; one complete new event provenance chain; moderation positive/negative fixtures; collection peak RSS/CPU.
7. **Bugs requiring code changes:** H1 requires explicit rustls provider installation. H2 requires documented blocklist provisioning, though an empty file is sufficient.
8. **Misconceptions requiring reinterpretation:** an HTTP-healthy process does not imply its background collector is healthy.
9. **What would change the verdict:** the same isolated test after the provider fix, with a new signed upstream event present in canonical storage and returned over HTTP while zero AWS calls remain.
10. **Recommended sequence:** fix only provider initialization; rerun against a fresh isolated DB copy with the empty blocklist and absent backend; prove relay→canonical→HTTP; then test retention/moderation and resource bounds. Do not change production or combine this with a new database snapshot before that readout.

-- Pi/gpt-5.6-sol
