# Deployment notes

The commands are in [README.md](../README.md). The resource definition in
[`infrastructure/cloudformation/meatybroth.yaml`](../infrastructure/cloudformation/meatybroth.yaml)
is the source of truth.

## Cost estimate

As of 2026-09-13, one continuously running deployment in `us-east-1` has an
estimated list price of **USD 21.73/month**, before tax and data transfer:

- `t3.small`: USD 0.0208/hour × 730 hours = USD 15.18/month.
- 30 GiB `gp3`: USD 0.08/GiB-month × 30 = USD 2.40/month.
- One public IPv4 address: USD 0.005/hour × 730 = USD 3.65/month.
- One Route53 hosted zone: USD 0.50/month.

The domain is separate: `.com` registration and renewal were each USD 16/year
when checked. DNS queries, outbound data, tax, snapshots, credits, and T3
Unlimited CPU surplus charges are not in the estimate. This is a list-price
estimate, not a measured bill; check Cost Explorer for actual charges.

Price provenance:

- AWS Price List API, queried 2026-09-13: EC2 SKU `QA3NBPZEQKZ2K9AR`
  (`t3.small`, Linux, shared tenancy, on-demand) and EBS SKU
  `JG3KUJMBRGHV3N8G` (`gp3`, `us-east-1`).
- [Amazon VPC pricing](https://aws.amazon.com/vpc/pricing/) states USD 0.005
  per in-use or idle public IPv4 address-hour.
- [Route53 pricing](https://aws.amazon.com/route53/pricing/) gives the first 25
  hosted zones as USD 0.50 per zone-month.
- [T3 instances](https://aws.amazon.com/ec2/instance-types/t3/) documents CPU
  surplus-credit charges in Unlimited mode.

## Resource and data lifecycle

The stack creates one VPC, public subnet, EC2 instance, encrypted 30 GiB root
volume, Elastic IP, Route53 A record, security group, and an SSM instance role.
Ports 80 and 443 are public; SSH is not. Caddy terminates HTTPS and proxies to
the loopback-bound reader.

The SQLite corpus is disposable. Deleting the stack deletes the root volume.
The deployment command is create-only; replacing the stack rebuilds the corpus.
A non-destructive application update path is not implemented.

`just deploy-prepare` creates a change set for review; `just deploy` executes a
first deployment; and `just deploy-health` checks the host services, public
HTTPS, and a non-empty corpus. That health check does not prove complete or
fresh relay coverage.

The 30-day post policy bounds logical retention, not forensic erasure from
SQLite free pages, WAL files, logs, or old EBS snapshots. The stack does not
create snapshots.
