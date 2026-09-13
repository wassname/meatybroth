# AWS Titan login preflight

Observed 2026-09-13 in the COI Incus sandbox. No `InvokeModel`, IAM mutation, deployment, purchase or paid API call was made.

## CLI repair

- Existing `/snap/bin/aws -> aws-cli.aws` returned no output and exit 120. Earlier `strace` isolated `EACCES` while snap tried to execute `snap-confine`; this occurs before AWS CLI or authentication starts. See `2026-09-13_aws-cli-local-diagnostic.log`.
- Downloaded the official x86-64 ZIP and detached signature from `https://awscli.amazonaws.com/` without piping anything to a shell.
- Extracted the AWS CLI Team public key from the [AWS installation documentation](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html).
- Verified key fingerprint `FB5D B77F D5C1 18B8 0511 ADA8 A631 0ACC 4672 475C` and a good detached signature on the ZIP. ZIP SHA-256: `617845f42577b8c1deba29eb5195ced529ee98241ccf4dad922745a287722af4`.
- Installed outside the workspace at `/usr/local/bin/aws`: `aws-cli/2.36.44 Python/3.14.6 Linux/7.0.0-31-generic exe/x86_64.ubuntu.24`.

## Existing temporary login

`/usr/local/bin/aws sts get-caller-identity --profile cds-login --region us-west-2` returned:

- account: `275713940406`
- ARN: `arn:aws:iam::275713940406:user/wassname100`
- user ID: `AIDAUAMOU3O3O7PGOPPIO`

No login cache, token, environment credential or credential file was printed or copied. The session was preserved; no logout was run.

## Unpaid Bedrock metadata checks

`get-foundation-model` in `us-west-2` returned:

- model ARN: `arn:aws:bedrock:us-west-2::foundation-model/amazon.titan-embed-text-v2:0`
- name/provider: `Titan Text Embeddings V2` / `Amazon`
- lifecycle: `ACTIVE`
- inference: `ON_DEMAND`, input `TEXT`, output `EMBEDDING`, streaming unsupported

`get-foundation-model-availability` returned:

- authorization: `AUTHORIZED`
- agreement: `AVAILABLE`
- entitlement: `AVAILABLE`
- region: `AVAILABLE`

These metadata responses establish identity, regional model availability and authorization status. They do not establish request payload compatibility, successful inference, billing behavior or the application's USD 5 ledger limit.

The app worker received: `PATH=/usr/local/bin:$PATH`, `AWS_PROFILE=cds-login`, and `AWS_REGION=us-west-2`. That worker alone owns the authorized capped invocation.

-- Pi/gpt-5.6-sol
