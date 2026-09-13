# Bedrock deployment handoff

Status: review candidate only. No stack, IAM resource, model invocation, purchase, or paid API call was made.

## Prepared file

[`infrastructure/deployment/meatybroth.yaml`](../../infrastructure/deployment/meatybroth.yaml) is a sibling Rust deployment candidate derived from the current public template. It does not replace or edit the Python deployment source.

The candidate:

- requires an explicit public Rust `SourceRepository`, a pushed 40-character `SourceRevision`, and `PaidBedrockApproval=I_APPROVE_USD_5_SETUP_AND_USD_5_MONTHLY`; none has a default;
- keeps the existing VPC, encrypted disposable root disk, Route53, HTTPS-only security group, SSM role, IMDSv2 and no-SSH design;
- adds only `bedrock:InvokeModel` on `arn:${AWS::Partition}:bedrock:us-west-2::foundation-model/amazon.titan-embed-text-v2:0`, with an `aws:RequestedRegion=us-west-2` condition;
- gives the Rust host process credentials through its EC2 role. It creates no key and stores no credential;
- builds the locked Rust source and starts the single reader/collector with `cargo run --locked --release` under systemd;
- passes the application owner's exact production settings: backend `bedrock`, Titan V2, 512 dimensions, normalization enabled, `us-west-2`, and USD 5.00 total/monthly application limits;
- binds the application to `0.0.0.0:8088`, without opening 8088 in the security group. A checksum-pinned Caddy container exposes only ports 80/443.

CloudFormation requires the approval parameter before it can create the stack. IAM cannot enforce a dollar ceiling. The application's durable request ledger must enforce both limits before every invocation.

## Blocking data-lifecycle issue

The database, vectors and request-cost ledger are on the instance's root volume, which still has `DeleteOnTermination: true`. Instance or stack replacement therefore loses the corpus and the evidence of prior Bedrock spending. A new empty ledger would make the application limits count from zero and could permit more spending than the intended cumulative cap.

Do not apply this candidate as a replacement until the deployment owner defines and tests a backup/restore migration. At minimum: stop the writer, checkpoint SQLite's WAL, copy and verify `events.sqlite` plus any required side files to encrypted retained storage, restore it on a fresh host, and compare event/vector/request-ledger counts before deleting the old volume. The template does not implement that storage expansion tonight.

## Checks performed

- [`slop/verification/2026-09-13_deployment-template-offline-validation.log`](../verification/2026-09-13_deployment-template-offline-validation.log): YAML/intrinsic parse, required-parameter gate, exact IAM action/resource/region, no access-key strings, ingress, all nine environment names, pinned checkout, rendered UserData `bash -n`, systemd syntax, and Caddy 2 config.
- [`slop/verification/2026-09-13_deployment-release-build.log`](../verification/2026-09-13_deployment-release-build.log): clean commit `724c0e1d13c2158340ff1772fc8789d3ca94fd03` builds in release mode with stable Rust 1.88 and its committed lockfile. App-owner MiniLM/Bedrock changes were uncommitted and are not covered; rebuild their final pushed SHA.
- Original deployment files remained unchanged. At review time their SHA-256 values were `a0d1726…32baf` for `infrastructure/cloudformation/meatybroth.yaml` and `52bc0d7…04571` for `scripts/deploy_cloudformation.sh`.

AWS `validate-template` was not run. The local snap AWS wrapper fails before AWS CLI starts; the observation is saved in [`slop/verification/2026-09-13_aws-cli-local-diagnostic.log`](../verification/2026-09-13_aws-cli-local-diagnostic.log). No claim is made that the account permits Titan access, the stack creates successfully, the host bootstraps, or `InvokeModel` works.

## Deployment-owner steps

1. Finish and review the application-side Bedrock transport and both fail-before-call budget checks. Keep local MiniLM as a distinct model space: `minilm`, `sentence-transformers/all-MiniLM-L6-v2`, 384 dimensions, normalized. Do not query Titan vectors with MiniLM or the reverse.
2. Resolve the root-volume backup/restore blocker above and rehearse it before any instance or stack replacement.
3. Move the reviewed Rust tree and this candidate into the intended public repository. The sibling currently has no Git remote. Push the exact reviewed commit; do not point `SourceRepository` at the old Python tree.
4. Resolve dependencies under the repository's eight-day publication hold and commit that `Cargo.lock`. The AL2023 host uses stable `/usr/bin/cargo` only with `--locked`: this prevents a new resolution but does not itself enforce publication age. The saved clean build reports that distinction explicitly.
5. Update the existing create/change-set wrapper for the new required repository and approval parameters. Its current source checks require Python/Docker inputs and therefore must not be reused unchanged.
6. On the deployment owner's host, use AWS CLI 2.32 or newer. Authenticate with `aws login --profile cds-login --region us-west-2`, then require account `275713940406` from `aws sts get-caller-identity --profile cds-login`. Do not create a permanent access key.
7. Only after the user approves both USD 5 caps, create a change set with `CAPABILITY_IAM` and the exact approval literal. Review the role policy, source URL/SHA, UserData and resource replacement before execution.
8. Run AWS `validate-template`, then verify the final pushed SHA builds on AL2023, `systemctl status meatybroth`, Caddy HTTPS, `/status`, restored database/vector/ledger counts, the effective instance-role ARN, and the exact model ARN. A separately authorized one-request probe is still needed to establish regional model access and response shape; do not use a corpus backfill as the probe.

For local development, MiniLM needs no AWS login. A developer testing Bedrock later should use the same temporary `cds-login` session and a separately reviewed identity policy scoped to the exact Titan ARN; no credentials belong in `.env` or the repository.

-- Pi/gpt-5.6-sol
