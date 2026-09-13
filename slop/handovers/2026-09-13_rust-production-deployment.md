# Rust production deployment handover

Status 2026-09-13T15:24Z: deployment authorized but paused. Old production is still serving. No CloudFormation, instance, volume, Caddy-upstream or application-service change occurred.

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

Confirm the authorization host is `us-east-1.signin.aws.amazon.com`; then make one serial unpaid STS call after the 15-minute refresh boundary. Alternate issuer region is a test derived from aws-cli#10267, not an established fix. Until then, make no AWS or production mutation.

Expected identity: account `275713940406`, `arn:aws:iam::275713940406:user/wassname100`. Titan metadata in us-west-2 was ACTIVE/AUTHORIZED/AVAILABLE. Do not log tokens or cache contents.

## Off-host artifacts

Temporary bundle: `/tmp/meatybroth-deploy-d1d72fc/`

- consistent SDK snapshot: `events.sqlite`, 194,793,472 bytes, integrity OK; 7,602 signed events, 149,923 social edges, `sqlite_stat1`, MiniLM space, cached Titan119 and six Titan topics assigning all119
- runtime env: `READ_ONLY=1`, `DEFAULT_EMBEDDING=titan`, DB and address only; no embed backend or AWS variables
- pinned runtime base: `ubuntu:24.04@sha256:a61567bd31828687156d735ea8eb01ba4e37636e225dd6a48ba94136a70d9d61`

AL2023 native linking is invalid: `ort-sys` rc13 requires glibc ≥2.38 `__isoc23_strto*` and newer libstdc++; AL2023 has glibc2.34/GCC11. Use the Ubuntu runtime container, not an AL-native binary or an app feature change.

Proved container method on app `d1d72fc`: non-root user10001, read-only rootfs and DB file, dropped capabilities, no-new-privileges. SQLite WAL mode requires a writable directory even for read-only app access; mount a small writable `/data` tmpfs around the read-only `/data/events.sqlite` bind. Private smoke returned root/status/cached Titan topic/Similar HTTP200 and uncached Titan Meaning HTTP400 without AWS.

Latest built but not final app image:

- source `3ea764bf3203781741bcebdd0301b63804e21f2b`
- binary SHA-256 `ee4e86d5aa76d68ca8ae7b15a67b0a22821655944039601592e3ff0a1d0f18dd`
- image `meatybroth-rust:3ea764b`, ID `sha256:0f4ef63483ccab7064bef4f36ad1060e263633d397f08b14d92fa846c0a3f7e0`, 48,175,465 bytes

App owner is preparing one stable follow-up after Similar/about/protocol-telemetry review. Do not rebuild each UI commit. For the final SHA: verify app source files match that commit, run `cargo +stable build --locked --release` off-host, update the Docker label, build with `--network=none --pull=false`, then repeat the exact private cached-Titan smoke. Stable Cargo ignores the age configuration, but `--locked` prevents resolution; retain the app owner's age-held lockfile.

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
