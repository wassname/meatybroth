# Rust production deployment handover

Status 2026-09-14T06:58+08: Rust revision `cb71d87af3c1d9d6eb7b9658c4c878f756561fca` is serving `https://meatybroth.com` and is the sole continuous SDK/Titan writer. Public Status, Topics, cached Meaning and Similar return HTTP 200. The instance role completed ledgered Titan calls with no profile or static credentials. The old Python web, collector and read-only Rust container remain available for rollback; they do not write to the continuous embedding ledger. CloudFormation, instance and volume were not replaced.

## Production instance IAM

At 20:57Z, the existing role `meatybroth-exp-wassname-InstanceRole-y6KZvcRn9Q6K` received inline policy `MeatybrothTitanV2InvokeUsWest2`. It allows only `bedrock:InvokeModel` on `arn:aws:bedrock:us-west-2::foundation-model/amazon.titan-embed-text-v2:0`. No wildcard action/resource, new role, new profile or instance change was made. IAM Access Analyzer returned no findings. IAM's policy simulator did not model the Bedrock resource and returned a placeholder implicit deny; this is not invocation evidence.

CloudFormation change set `meatybroth-titan-role-20260913` was generated from the live stack template. It contained one change only: `InstanceRole` Modify, property `Policies`, replacement `False`, recreation `Never`. Execution completed with stack status `UPDATE_COMPLETE` at 21:00:53Z; the exact policy read back afterward. Saved applied template: `slop/deployment/2026-09-13_live-stack-template-with-titan.yaml`.

The applied template still bootstraps `wassname/meatybroth` with its Python Docker Compose service on instance replacement. It records the current IAM/metadata state but is not a repeatable Rust deployment template. A replacement Rust bootstrap must treat `events.sqlite` and its sibling `blocklist.txt` as one versioned input set and fail on either hash mismatch. Fixing bootstrap remains required before any stack or instance replacement; it does not affect the existing role proof.

### Continuous-deployment input checklist

- Transfer the canonical SQLite backup and canonical `blocklist.txt` together.
- Record source path, destination path, byte count and SHA-256 for both in the deployment manifest.
- Verify both destination hashes before starting the writer. Missing moderation input is a deployment failure.
- An empty blocklist is valid only when the canonical source is present, zero bytes and hashes to `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`. Do not create an empty fallback when the source is absent.

SSM command `fae82b3f-f0b1-46ea-83a1-ac22b03f18ec` unset AWS credential/profile/region variables, obtained the IMDSv2 role name and used default-chain STS. It returned account `275713940406` with the expected assumed-role class. No model invocation was made outside the application ledger.

## User-visible limitations

Titan Meaning, Similar and Topics work, and the header now shows Semantic/Titan availability on all pages. MiniLM is not configured. A full 100-card New response was 6.57 MB and took 15.09 seconds in the first public check; this remains the clearest user-visible performance limitation.

## Evidence and retained production state

- Full action log: `slop/verification/2026-09-13_production-rust-deployment.log`
- Social planner review: `slop/verification/2026-09-13_social-query-plan-review.md`
- Stack `meatybroth-exp-wassname`, us-east-1; EC2 `i-0a99d95567fc8d891`, t3.small, SSM Online, public `34.192.250.76`
- Verified in-place rollback backup: `/opt/meatybroth-backups/pre-rust-20260913T133421Z` on the production instance. SQLite backup and copy-back restore both passed integrity/count checks. DB SHA-256 `c9d143edfee24f84b2914f786df8e08413c66daea9b13d9d994342ee583d7811`.
- The backup shares the root volume; do not replace the instance/volume or update/delete the stack.
- AL2023 signed Rust/Cargo/GCC packages were installed, but no build or service change ran on the instance.

## AWS authentication resolution

Use official `/usr/local/bin/aws` v2.36.44. Do not use `/snap/bin/aws`, logout, delete cache or create keys.

Two user logins produced valid 15-minute access credentials, but proactive refresh failed about five minutes before expiry with `CreateOAuth2Token INVALID_REQUEST`. Cache/session hash, owner/mode, DPoP key presence, refresh-token presence, profile association and UTC clock were correct. The second failure was sequential, not a concurrent refresh.

AWS CLI issue #10613 confirms the observed failure mode: Sign-In refresh tokens must be redeemed in their issuer region, while the CLI refreshes through the current request region. A bounded decode of only the cached ID token's nonsecret `iss` claim proves the successful login issuer was `us-east-2`. The Titan workload refreshed in `us-west-2`; the later unpaid STS retry refreshed in `us-east-1`. Both were region mismatches. Initial access working and refresh failing at the proactive boundary matches the AWS-documented behavior. An unpaid STS request through the proven `us-east-2` issuer succeeded, confirming region routing as the cause. Deployment used issuer-aware in-memory temporary credentials for `us-east-1` resource calls; no credential was printed, saved or added to the application.

The human login is not used by the production application. Continuous Titan will use the existing EC2 role through IMDS/default credential discovery, with no profile, static key or human refresh.

Expected identity: account `275713940406`, `arn:aws:iam::275713940406:user/wassname100`. Titan metadata in us-west-2 was ACTIVE/AUTHORIZED/AVAILABLE. Do not log tokens or cache contents.

## Current continuous artifact

- source: `cb71d87af3c1d9d6eb7b9658c4c878f756561fca`
- release binary SHA-256: `3e35b1adae0bfbe1f16b138b507a0de53d95483ead846965512f7c4cceae25c4`
- local image digest: `sha256:3132bd9d9a21abbc9334b538ee25da14e70c12d1347c73ec9e840a19157bc037`
- EC2 image config ID: `sha256:09c526852a0dfc373fcebcbfae0ad155a950c2c038cb28e1663d7ccfbd9cec52`
- CA bundle SHA-256: `ecd9dc38bc3efb7dbd6431f57e29d2f8d6a0f0d211e1464b3fef2cbfe266fcd2`
- canonical data: `/opt/meatybroth-rust/continuous/events.sqlite` and sibling `blocklist.txt`
- container: `meatybroth-rust-next`, user10001, read-only root, all capabilities dropped, no-new-privileges, restart `unless-stopped`, stop timeout 180 seconds
- public upstream: `meatybroth-rust-next:8088`; prior Caddy file: `/opt/meatybroth/Caddyfile.pre-continuous-5dca648`
- manifest: `slop/deployment/2026-09-14_continuous-rust-manifest.txt`

The app handles SIGTERM/INT by blocking new provider windows and draining the current provider future and ledger. Docker container stop timeout and daemon shutdown timeout are both 180 seconds. Continue to use a verified no-inflight boundary for planned upgrades.

## Earlier read-only artifact

Earlier local bundle: `/tmp/meatybroth-deploy-1d36733/`. The image and database were transferred through two AES256-encrypted, public-blocked S3 objects with 15-minute presigned downloads; exact hashes were verified on the instance and both objects were deleted immediately. Build and smoke evidence is in `slop/verification/2026-09-13_production-rust-deployment.log`.

- source: `1d367334fad3aec55a4a8331e926e2259d87a046`, built from an isolated exact worktree with the committed lockfile
- binary SHA-256: `2cd80dbef4915f7214ef6e344b7a8607a79c7c94d351d26ffcf43086d0901a61`
- image: `meatybroth-rust:1d36733`, local content digest `sha256:14f1a88b1d8e95e3c8d790b466e2c2548890477f8b7ad91e531e3a9adaa3e7e3`, archive config ID `sha256:a348a116e77a8c24931630d27e0cf2fa883fc7ee01219890265795812ed0bfef`, 48,197,968 bytes
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

## Applied reversible public upstream change

1. Fresh rollback backup `/opt/meatybroth-backups/pre-rust-final-20260913T182229Z`: online SQLite backup integrity `ok`, copy-back restore proof passed, counts `posts=8104`, `follows=33804`, DB SHA-256 `6d5c6e78d6e51cfcccf17cfe584f7097769cef6e7bb6d4dee666dee212d1a700`. The earlier backup remains.
2. Exact image and SDK DB hashes were verified after private transfer. The Rust container is on `meatybroth_default`, published only at host `127.0.0.1:8088`, user10001, read-only root/DB, `/data` tmpfs, all capabilities dropped, no-new-privileges, no AWS or embedding-backend environment.
3. The first Rust start failed explicitly because the deployment env used wrong variable names; it was replaced with the exact artifact env names before any public change. Initial bounded checks missed Social's larger SQLite sort. Public Social then returned 500 with SQLite `DiskFull` while `/tmp` was a 16 MiB tmpfs. Caddy was immediately restored to the old upstream; a complete old Social response returned HTTP 200 with 49,965 bytes.
4. Direct monitoring proved the spill was a deleted-open `/tmp/etilqs_*` file while host disk had 27 GiB free and `/data` stayed at 32 KiB. Rust now uses dedicated disk-backed `/opt/meatybroth-rust/tmp`, owner10001 mode0700, bind-mounted writable at `/tmp`; the canonical DB remains read-only. Private Social cold/repeat both returned 200 with identical 246,053-byte bodies, prior controls passed, temp files returned to zero and restart count stayed zero.
5. `/opt/meatybroth/Caddyfile.pre-rust-final` retains the prior upstream. Existing Caddy was validated and reloaded after changing only `web:8081` to `meatybroth-rust:8088`.
6. Independent final public checks passed: Social cold/repeat each HTTP 200 and 246,053 bytes; root/status/Titan Topic/Titan Similar/eligible contexts HTTP 200; exact telemetry contexts 404 and Similar 400; unavailable Meaning and MiniLM direct URLs 400.
7. Rollback: copy `Caddyfile.pre-rust-final` over `Caddyfile`, validate/reload `meatybroth-caddy-1`, then remove only `meatybroth-rust`. Old web, collector, data volume and Caddy remain running.

Cached Titan119 is explicitly a partial NIP-13 PoW-biased cohort. Production status must say partial; do not call it retained-corpus parity or enable recurring Bedrock ingestion.

-- Pi/gpt-5.6-sol
