# Native Bedrock transport feasibility and result

## Decision

Use the maintained `aws-sdk-bedrockruntime` client for the authorized one-off Titan backfill. The existing AWS CLI transport took about 3.5 seconds per note, projecting 13.37 hours for the frozen 13,755-post backlog. The native path completed ten notes in 9.57 seconds including process setup and one topic rebuild; after subtracting the fixed rebuild, incremental requests were about 0.5 seconds each. This projected about 1.9 hours and made completion inside the temporary authorization window plausible.

No parallel database writer was introduced. Collection was stopped, the public review process uses a frozen read-only database copy, and the native backfill reserves, awaits and completes one request at a time.

## Maintained API and credentials

## `aws_config::load_defaults` — AWS SDK for Rust documentation — [docs.rs](https://docs.rs/aws-config/1.12.0/aws_config/fn.load_defaults.html)

> Load default configuration chain
>
> This loads the default config chain with the given behavior version. This is equivalent to `aws_config::defaults(version).load().await`.

## AWS SDK for Rust credentials — AWS documentation — [AWS](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/credproviders.html)

> The SDK for Rust provides a default credential provider chain that searches for credentials in a defined order.

The production process does not ask the SDK to interpret the login profile. A shell refreshes `cds-login` through its issuer region, `us-east-2`, using `aws configure export-credentials`; the output is evaluated only in process memory and is neither printed nor written. `AWS_PROFILE` is then unset. The native Bedrock client loads those environment credentials and sends requests to `us-west-2`.

The client uses:

- `aws-config` 1.12.0;
- `aws-sdk-bedrockruntime` 1.143.0;
- explicit `BehaviorVersion::latest()`;
- standard retries with `max_attempts=1`;
- 30-second operation and operation-attempt timeouts;
- `InvokeModel` with JSON `Blob`, Titan v2 model ID, 512 dimensions and normalization;
- the existing pre-call reservation and post-response ledger.

Resolution used the configured eight-day Cargo publication hold. The repository pins nightly Rust and resolved Rust 1.100, above the SDK's 1.94.1 minimum. A misleading initial failure used the machine's default stable 1.88 outside the repository toolchain; it did not apply to the project build.

## Cost and first success

The first native success embedded event `00000002ef29…`:

- 28 input tokens;
- 560 nano-US-dollars = US$0.000000560;
- 5.23 seconds for the one-item process including topic rebuild;
- Titan cache moved from 119 to 120 posts;
- cumulative ledger became 7,275 tokens and US$0.000145500.

A ten-item rate sample then completed in 9.57 seconds including another rebuild.

Evidence:

- `slop/verification/2026-09-13_titan-native-first-item-07.log`
- `slop/verification/2026-09-13_titan-native-first-item-ledger.log`
- `slop/verification/2026-09-13_titan-native-ten-item-rate.log`

## Accounting failure found during the first long segment

The initial segment later encountered a two-byte input for which Titan reported three tokens. The prior reservation used `input_bytes` as the token upper bound, so completion marked the known response uncertain rather than exceed the reservation. The event was not retried. The correction reserves `input_bytes + 8` token slots and pending selection skips unresolved reserved/uncertain events. The ledger retains the one uncertain item visibly and charges its reservation against both caps.

## Maintenance and footprint

Replacing the subprocess transport changed production source by 37 additions and 55 deletions in `src/embed.rs` plus one changed call site: net 18 fewer Rust lines. It added 881 Cargo-lock lines and removed 50. The stripped release binary grew from 51,433,728 to 67,768,200 bytes: +16,334,472 bytes (+31.76%).

This is a real binary/dependency cost, not a smaller artifact. The trade is accepted for this one-off because it removes per-note process startup, uses the maintained AWS request/signing stack, preserves the existing serialized ledger, and changes the projected completion from longer than the authorization window to comfortably inside it.

The debug prototype's 177 MiB file is not used as a production footprint comparison.

— Pi/gpt-5.6-sol
