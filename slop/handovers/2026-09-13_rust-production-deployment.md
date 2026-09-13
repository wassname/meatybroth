# Rust production deployment handover

Status 2026-09-13T17:50Z: final deployable revision `1d367334fad3aec55a4a8331e926e2259d87a046` and a moderation-clean SDK snapshot passed the hardened off-host smoke. Deployment remains paused for a human-attended AWS login issuer-region test. Old production is still serving. No CloudFormation, instance, volume, Caddy-upstream or application-service change occurred.

## User-visible limitations

This is a cached-Titan deployment candidate, not complete embedding-search functionality. The UI disables Meaning as `Meaning unavailable`, omits MiniLM and selects Titan. Direct Meaning and MiniLM URLs still return explicit HTTP 400. Cached-Titan Similar and Topic work; functional Meaning search does not.

## Evidence and retained production state

- Full action log: `slop/verification/2026-09-13_production-rust-deployment.log`
- Social planner review: `slop/verification/2026-09-13_social-query-plan-review.md`
- Stack `meatybroth-exp-wassname`, us-east-1; EC2 `i-0a99d95567fc8d891`, t3.small, SSM Online, public `34.192.250.76`
- Verified in-place rollback backup: `/opt/meatybroth-backups/pre-rust-20260913T133421Z` on the production instance. SQLite backup and copy-back restore both passed integrity/count checks. DB SHA-256 `c9d143edfee24f84b2914f786df8e08413c66daea9b13d9d994342ee583d7811`.
- The backup shares the root volume; do not replace the instance/volume or update/delete the stack.
- AL2023 signed Rust/Cargo/GCC packages were installed, but no build or service change ran on the instance.

## AWS authentication blocker

Use official `/usr/local/bin/aws` v2.36.44. Do not use `/snap/bin/aws`, logout, delete cache or create keys.

Two user logins produced valid 15-minute access credentials, but proactive refresh failed about five minutes before expiry with `CreateOAuth2Token INVALID_REQUEST`. Cache/session hash, owner/mode, DPoP key presence, refresh-token presence, profile association and UTC clock were correct. The second failure was sequential, not a concurrent refresh. Leading diagnosis is an AWS Sign-In refresh regression; this is not proved.

Next human-attended test, exactly:

```bash
/usr/local/bin/aws login --profile cds-login --region us-east-1
/usr/local/bin/aws sts get-caller-identity --profile cds-login --region us-west-2
```

Confirm the authorization host is `us-east-1.signin.aws.amazon.com`; then make one serial unpaid STS call after the 15-minute refresh boundary. Alternate issuer region is a test derived from aws-cli#10267, not an established fix. At 15:49Z the parent retried STS with `--region us-east-1` against the existing login issuer; refresh still returned `INVALID_REQUEST` request 254. That changed the STS resource region, not the login issuer, so it did not test the proposed login command. Do not poll authentication repeatedly. Until a human attends the issuer-region test, make no AWS or production mutation.

Expected identity: account `275713940406`, `arn:aws:iam::275713940406:user/wassname100`. Titan metadata in us-west-2 was ACTIVE/AUTHORIZED/AVAILABLE. Do not log tokens or cache contents.

## Final off-host artifacts

Final local bundle: `/tmp/meatybroth-deploy-1d36733/`. It has not been transferred. Build and smoke evidence is in `slop/verification/2026-09-13_production-rust-deployment.log`.

- source: `1d367334fad3aec55a4a8331e926e2259d87a046`, built from an isolated exact worktree with the committed lockfile
- binary SHA-256: `2cd80dbef4915f7214ef6e344b7a8607a79c7c94d351d26ffcf43086d0901a61`
- image: `meatybroth-rust:1d36733`, ID `sha256:14f1a88b1d8e95e3c8d790b466e2c2548890477f8b7ad91e531e3a9adaa3e7e3`, 48,197,968 bytes
- runtime base: `ubuntu:24.04@sha256:a61567bd31828687156d735ea8eb01ba4e37636e225dd6a48ba94136a70d9d61`
- moderation-clean SDK snapshot: 387,764,224 bytes, SHA-256 `4aabaecc632017f11d5dcb20de86b7b052f3e5226349c13637e76a2b7b55ec32`, integrity OK; 16,403 canonical events, 12,510 reader posts, 289,884 social edges, 11,595 MiniLM vectors, 119 cached Titan vectors and six Titan topics assigning all 119
- runtime env: `READ_ONLY=1`, `DEFAULT_EMBEDDING=titan`, DB and address only; no embed backend or AWS variables
- exact telemetry fixtures `3516…1995` and `56e0…bf53`: canonical event present, reader-ineligible, zero vector/chunk/topic rows, Context 404, Similar 400
- eligible JSON control `84c9…dcb4`: Context 200 and one MiniLM vector/chunk/topic row; its MiniLM Similar is intentionally 400 because production does not configure MiniLM
- eligible cached-Titan control `00000004…f710`: Context 200 and Titan Similar 200

The older `3970a76` and `bc08fc7` bundles are superseded and must not be deployed.

AL2023 native linking is invalid: `ort-sys` rc13 requires glibc ≥2.38 `__isoc23_strto*` and newer libstdc++; AL2023 has glibc2.34/GCC11. Use the Ubuntu runtime container, not an AL-native binary or an app feature change. Stable Cargo ignores the age configuration, but `--locked` prevented resolution; the app owner's age-held lockfile was retained.

Compatibility image smoke used non-root user10001, read-only rootfs/database, dropped capabilities, no-new-privileges and host-only port `127.0.0.1:18089`. SQLite WAL mode requires a writable directory even for read-only app access; a 16 MiB `/data` tmpfs around the read-only `/data/events.sqlite` bind fixed the initially detected open failure. Root returned 100 cards/HTTP200 in 0.30 s; status 200/0.11 s; cached Titan topic 37 cards/200/0.29 s; cached Titan Similar 100 cards/200/0.44 s; uncached Titan Meaning returned the expected 400 without AWS.

A private Caddy canary on an existing-network equivalent resolved `meatybroth-rust:8088` and returned root/status/Titan topic HTTP200 in 0.12–0.32 s. Dropping every capability initially prevented execution because the Caddy binary carries a file capability; the verified minimum is `--cap-drop ALL --cap-add NET_BIND_SERVICE`.

## Intended reversible public upstream change

1. Make a fresh consistent local SQLite backup after the stable app checkpoint and verify Titan topics/membership plus `sqlite_stat1`.
2. Build/save the final pinned Ubuntu image off-host. Transfer the image and DB through a bounded private channel only; no transfer has occurred. An existing private same-region S3 deployment bucket may be used after auth review, with encryption, exact SHA verification and immediate object deletion.
3. On production, preserve the old Docker web/collector/Caddy containers. Load the final image and install the SDK DB separately from the old Python volume.
4. Start Rust container on existing `meatybroth_default` network, bind `0.0.0.0:8088` only inside the container and publish host `127.0.0.1:8088`. Use read-only rootfs/DB, writable `/data` tmpfs, user10001, dropped capabilities and no AWS environment.
5. Test host loopback. Start a temporary Caddy container on the existing network and host `127.0.0.1:18080` to prove `meatybroth-rust:8088` DNS/upstream before public reload.
6. Back up the current Caddyfile, change only its upstream from `web:8081` to `meatybroth-rust:8088`, validate and reload the existing Caddy container. Do not replace Caddy or its certificate/config volumes.
7. Verify public root/status/Titan topic/Similar and uncached Titan Meaning400. If any check fails, restore the old Caddyfile/reload and remove only the new Rust container; old services and data remain.

Cached Titan119 is explicitly a partial NIP-13 PoW-biased cohort. Production status must say partial; do not call it retained-corpus parity or enable recurring Bedrock ingestion.

-- Pi/gpt-5.6-sol
