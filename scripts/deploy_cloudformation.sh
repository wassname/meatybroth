#!/usr/bin/env bash
# Deploy the Meaty Broth reader: create a CloudFormation change set and, with
# --execute, wait for it to be created, inspect it and apply it. The USER runs
# this from their authenticated AWS shell; agents test it only against mocked
# aws/git CLIs. Requires an explicit account, region, stack name and a revision
# that is already on the expected origin repository.
set -euo pipefail

EXPECTED_REPO="github.com/wassname/meatybroth"

usage() {
  echo "usage: $0 --account ACCOUNT --region REGION --stack-name meatybroth-exp-NAME --domain DOMAIN --source-revision FULL_SHA [--execute]" >&2
  exit 2
}

account="" region="" stack="" domain="" revision="" execute=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --account) account="$2"; shift 2 ;;
    --region) region="$2"; shift 2 ;;
    --stack-name) stack="$2"; shift 2 ;;
    --domain) domain="$2"; shift 2 ;;
    --source-revision) revision="$2"; shift 2 ;;
    --execute) execute=1; shift ;;
    *) usage ;;
  esac
done
[[ -n "$account" && -n "$region" && -n "$stack" && -n "$domain" && -n "$revision" ]] || usage
[[ "$stack" == meatybroth-exp-* ]] || { echo "refusing: stack name must start with meatybroth-exp-" >&2; exit 1; }
[[ "$domain" =~ ^[a-z0-9][a-z0-9.-]*[a-z0-9]$ && "$domain" == *.* ]] \
  || { echo "refusing: invalid --domain $domain" >&2; exit 1; }
[[ "$revision" =~ ^[0-9a-f]{40}$ ]] || { echo "refusing: --source-revision must be a full 40-char SHA" >&2; exit 1; }
cd "$(dirname "$0")/.."

# The EC2 bootstrap clones the revision from the origin remote: it must exist
# there, and on the repository the template actually clones. Read-only fetch;
# nothing here pushes.
# Accept the exact repository under its HTTPS or SSH spellings only.
origin=$(git remote get-url origin)
case "$origin" in
  https://github.com/wassname/meatybroth | https://github.com/wassname/meatybroth.git |\
  git@github.com:wassname/meatybroth | git@github.com:wassname/meatybroth.git) ;;
  *) echo "refusing: origin $origin is not the template's repository ($EXPECTED_REPO)" >&2; exit 1 ;;
esac
git fetch origin --quiet || { echo "refusing: cannot reach origin; revision must be remotely available" >&2; exit 1; }
git merge-base --is-ancestor "$revision" origin/main \
  || { echo "refusing: revision $revision is not on origin/main; publish it first (review before push)" >&2; exit 1; }
for path in Caddyfile Dockerfile docker-compose.yml meatybroth infrastructure pyproject.toml uv.lock; do
  git cat-file -e "$revision:$path" \
    || { echo "refusing: deployment input $path is absent from $revision" >&2; exit 1; }
done
# Deployment inputs must match the revision exactly, or the built app is not
# the reviewed source.
git diff --quiet "$revision" -- Caddyfile Dockerfile docker-compose.yml meatybroth infrastructure pyproject.toml uv.lock \
  || { echo "refusing: tracked deployment inputs differ from $revision; commit or deploy the exact reviewed source" >&2; exit 1; }
curl -fsS --max-time 15 \
  "https://raw.githubusercontent.com/wassname/meatybroth/$revision/Dockerfile" >/dev/null \
  || { echo "refusing: revision $revision is not publicly readable; EC2 cannot clone it" >&2; exit 1; }

template=infrastructure/cloudformation/meatybroth.yaml
caller=$(aws sts get-caller-identity --region "$region" --query Account --output text)
[[ "$caller" == "$account" ]] || { echo "refusing: credentials are for account $caller, not $account" >&2; exit 1; }
hosted_zone_id=$(aws route53 list-hosted-zones-by-name --dns-name "$domain" \
  --query "HostedZones[?Name=='$domain.' && Config.PrivateZone==\`false\`].Id | [0]" --output text)
hosted_zone_id="${hosted_zone_id##*/}"
[[ -n "$hosted_zone_id" && "$hosted_zone_id" != "None" ]] \
  || { echo "refusing: no public Route53 hosted zone found for $domain" >&2; exit 1; }

# Fail early if an old Rusty Claw stack/target exists under this name.
# Create-only protection stays; but auth/network failures must not be mistaken
# for "stack missing" and silently proceed.
set +e
describe_err=$(aws cloudformation describe-stacks --region "$region" --stack-name "$stack" 2>&1)
describe_rc=$?
set -e
case $describe_rc in
  0) echo "refusing: stack $stack already exists; this script is create-only" >&2; exit 1 ;;
  *)
    if echo "$describe_err" | grep -q "ValidationError" && echo "$describe_err" | grep -qi "does not exist"; then
      : # genuinely missing stack: safe to prepare creation
    else
      echo "refusing: could not confirm stack absence (rc=$describe_rc); fix credentials/network before mutating:" >&2
      echo "$describe_err" >&2
      exit 1
    fi
    ;;
esac

change_set="$stack-changeset-$(date -u +%Y%m%dT%H%M%SZ)"
aws cloudformation create-change-set \
  --region "$region" --stack-name "$stack" --change-set-name "$change_set" \
  --change-set-type CREATE --capabilities CAPABILITY_IAM \
  --template-body "file://$template" \
  --parameters "ParameterKey=SourceRevision,ParameterValue=$revision" \
    "ParameterKey=DomainName,ParameterValue=$domain" \
    "ParameterKey=HostedZoneId,ParameterValue=$hosted_zone_id"

# create-change-set is asynchronous: wait, then inspect before any execution.
if ! aws cloudformation wait change-set-create-complete \
    --region "$region" --stack-name "$stack" --change-set-name "$change_set"; then
  echo "refusing: change set creation failed or timed out; reason:" >&2
  aws cloudformation describe-change-set --region "$region" --stack-name "$stack" \
    --change-set-name "$change_set" --query "[Status, StatusReason]" --output text >&2 || true
  exit 1
fi
status=$(aws cloudformation describe-change-set --region "$region" --stack-name "$stack" \
  --change-set-name "$change_set" --query "Status" --output text)
[[ "$status" == "CREATE_COMPLETE" ]] \
  || { echo "refusing: change set status $status is not executable" >&2; exit 1; }

if [[ -z "$execute" ]]; then
  echo "change set created; ready for your review (NOT executed): $change_set"
  echo "apply exactly this change set:"
  echo "  aws cloudformation execute-change-set --region $region --stack-name $stack --change-set-name $change_set"
  exit 0
fi

# Reached only with --execute: CREATE changes real resources.
aws cloudformation execute-change-set \
  --region "$region" --stack-name "$stack" --change-set-name "$change_set"
echo "change set executed; waiting for CREATE_COMPLETE (VPC+EC2 bootstrap, minutes)..."
aws cloudformation wait stack-create-complete --region "$region" --stack-name "$stack"
aws cloudformation describe-stacks --region "$region" --stack-name "$stack" \
  --query "Stacks[0].Outputs" --output text
echo "next: just deploy-health  (SSM checks + public HTTPS)"
