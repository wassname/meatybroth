# Private role-backed Titan production audit

- run: private `meatybroth-rust-next` on EC2
- app: `e1c8fbe89dcd62eb67792d1460fcbc5eff72ac0b`
- image: `sha256:0eaf6f3c4d9d7a0ff47798f415091389b00d17dbf2dd9ae091af536f650691f7`
- canonical handoff: `cabe5372bd11729bd33468c311e9e980e5e20ddfa9bd47c973943be473cecd02`
- evidence: `slop/verification/2026-09-14_e1c8fbe-private-writer.log`, SSM commands in the deployment verification log

## Summary

The private writer did not prove role-backed Titan invocation. It admitted 976 eligible posts, but every one of 64 Titan attempts ended `uncertain` with a transport dispatch failure. No new embedding was written. The writer was stopped and automatic restart disabled before public cutover.

The runtime image has no CA certificate bundle. This is highly likely (about 90%) to be the transport failure because the AWS SDK requires TLS to the Bedrock Runtime endpoint, while the Nostr SDK can use embedded webpki roots. The next image must contain a pinned CA bundle and the diagnostic app must expose the non-secret error source chain and stop after one failed provider call.

## Execution timeline

- 21:50:36Z: first private container start from the verified handoff.
- 21:50:37Z: reader HTTP server started.
- 21:50:43Z: collector exited because `blocklist.txt` was absent.
- 21:52:06Z: matching canonical empty blocklist supplied; container restarted.
- 21:52:10Z: all-relay inventory sync completed.
- 21:52:25Z onward: each attempted Titan call failed at dispatch and was recorded uncertain.
- 21:56:17Z: stop completed after the 30-second grace period; exit 137. Restart policy was changed to `no`.

## Ledger evidence

Handoff Bedrock state:

- succeeded: 9,018 requests, 72,300,580 nUSD actual
- uncertain: request 18,209, 60 nUSD actual
- reserved: requests 18,560–18,562, 491,520 nUSD held

Stopped private-run state:

- succeeded: 9,018, unchanged
- uncertain: 65 total
- new uncertain: 64
- new conservative held cost: 10,485,760 nUSD ($0.01048576)
- reserved: 3 Bedrock rows, unchanged
- newly admitted eligible posts: 976
- newly embedded admitted posts: 0

The three historical reserved rows and all uncertain rows remain held. No row was released or retried silently.

## Primary log evidence

> `collector exited: No such file or directory (os error 2)`

This came from the first start, before a provider request. Source inspection found the collector opens `blocklist.txt` beside the database. The deployment manifest now treats both files as required inputs.

After restart, the collector worked:

> `initial inventory sync complete relay=wss://relay.damus.io notes=5851`
>
> `initial inventory sync complete relay=wss://nos.lol notes=3298`
>
> `initial inventory sync complete relay=wss://relay.primal.net notes=2730`
>
> `initial inventory sync complete relay=wss://nostr.wine notes=11077`

Every embedding attempt then reported:

> `uncertain: Titan InvokeModel failed: dispatch failure`

The final recorded request was:

> `embedding request_id=18629 event=81a5cebb... uncertain: Titan InvokeModel failed: dispatch failure`

The full log contains 64 such uncertain request records. The application log used `Display`, so it did not retain the nested dispatch cause.

## Unpaid runtime diagnostics

Observed inside the exact runtime image:

> `ca_bundle=false`
>
> `dpkg-query: no packages found matching ca-certificates`

Previously, the hardened Docker network/user path successfully obtained an IMDSv2 token, the expected instance role and a credential document with status `Success`, without printing values. That proves reachability but not that the AWS SDK credential chain completed. The role policy permits only the exact Titan V2 InvokeModel ARN.

## Ranked diagnoses

1. **Missing runtime CA bundle — about 90%.** Directly observed and sufficient to explain TLS dispatch failure. Nostr WSS success does not refute it because that SDK can use embedded roots.
2. **AWS SDK credential-provider failure — about 5%.** Manual IMDS succeeds in the hardened network, but the app-level chain is not independently proven.
3. **DNS/TCP path failure — about 4%.** Plausible but less consistent with successful relay networking; exact Bedrock endpoint checks still need capture.
4. **IAM denial — about 1%.** A valid signed request with wrong authorization should normally return a service response, not a transport dispatch failure.

Counterfactual: if a CA-equipped diagnostic image still reports dispatch failure, the leading diagnosis is wrong; inspect the new source chain before any second retry. If it reaches AWS but IAM is wrong, expect an explicit authorization response and preserve that one request as uncertain until billing is reconciled.

## Next decision

1. Add a pinned CA bundle to the runtime package and record its digest.
2. Use the app diagnostic that logs the non-secret error source chain and disables further paid calls after the first transport failure.
3. Continue from the current failed-run database, not the original handoff, so its 64 uncertain holds and 976 admitted posts remain canonical.
4. Make one role-backed Titan request. Require one `succeeded` ledger row and one embedding before restoring continuous operation.
5. Measure all newly admitted eligible rows and one uncached then cached semantic query before public switch.

## Follow-up: cause confirmed and production continued

A CA-equipped `5dca648` image retained the same network, role and database. Ledgered Titan calls then succeeded. This confirms the missing CA bundle diagnosis.

The intended one-request diagnostic isolation did not work: `MEATYBROTH_EMBED_DEADLINE_EPOCH=0` was assumed to gate all pending embedding, but it does not gate `embed_recent_pending`. After one successful query preflight, ordinary concurrency ran until the process was stopped. The exact delta was 279 succeeded requests, 627,264 tokens and $0.01254528 actual cost. The stop needed SIGKILL after 30 seconds and added eight uncertain and two reserved rows. All history remains in the canonical remote database.

This result falsifies the claim in the earlier next-decision section that the first CA-equipped process would make one request. The excess calls were within the already authorized continuous monthly budget, but the diagnostic method failed its stated bound. Revision `5dca648` was then started once in steady-state and left running. Public Caddy routes to it.

— Pi/OpenAI
