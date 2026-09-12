#!/usr/bin/env bash
# Post-deployment health checks (user runs from their authenticated shell).
# CREATE_COMPLETE is not a healthy app: this verifies the account, stack and
# instance states, then checks the real services and served app on the host.
set -euo pipefail
usage() { echo "usage: $0 account=ACCOUNT region=REGION stack=STACK" >&2; exit 2; }
account="" region="" stack=""
for kv in "$@"; do
  case "$kv" in
    account=*) account="${kv#*=}" ;;
    region=*) region="${kv#*=}" ;;
    stack=*) stack="${kv#*=}" ;;
    *) usage ;;
  esac
done
[[ -n "$account" && -n "$region" && -n "$stack" ]] || usage

step() { echo "== $*"; }
expect_eq() { # expect_eq GOT WANT DESCRIPTION — exact equality
  [[ "$1" == "$2" ]] || { echo "FAIL: $3 (got: '$1', want: '$2')" >&2; exit 1; }
}

step "credentials belong to the account (before any SSM use)"
caller=$(aws sts get-caller-identity --region "$region" --query Account --output text)
expect_eq "$caller" "$account" "credentials account mismatch"

step "stack status (want CREATE_COMPLETE)"
status=$(aws cloudformation describe-stacks --region "$region" --stack-name "$stack" \
  --query "Stacks[0].StackStatus" --output text)
expect_eq "$status" "CREATE_COMPLETE" "stack not CREATE_COMPLETE"

instance=$(aws cloudformation describe-stacks --region "$region" --stack-name "$stack" \
  --query "Stacks[0].Outputs[?OutputKey=='InstanceId'].OutputValue" --output text)
[[ "$instance" =~ ^i-[0-9a-f]+$ ]] || { echo "FAIL: no InstanceId output (got: '$instance')" >&2; exit 1; }

step "instance $instance running + status ok"
aws ec2 wait instance-status-ok --region "$region" --instance-ids "$instance"
# AWS CLI --output text renders a two-element tuple as tab-separated (running\tok),
# not newline-separated. Read it as the documented tabular text output.
instance_status=$(aws ec2 describe-instance-status --region "$region" --instance-ids "$instance" \
  --query "InstanceStatuses[0].[InstanceState.Name, InstanceStatus.Status]" --output text)
IFS=$'\t' read -r istate ihealth <<< "$instance_status"
expect_eq "$istate" "running" "instance not running"
expect_eq "$ihealth" "ok" "instance status not ok"

step "SSM online"
for _ in $(seq 1 60); do
  ping=$(aws ssm describe-instance-information --region "$region" \
    --filters "Key=InstanceIds,Values=$instance" \
    --query "InstanceInformationList[0].PingStatus" --output text)
  [[ "$ping" == "Online" ]] && break
  sleep 5
done
expect_eq "$ping" "Online" "instance not online in SSM"

step "real service/app checks on the host"
remote='
set -e
# Compose resolves .env relative to its project directory; SSM starts in an
# implementation-dependent working directory, so use the deployed checkout.
cd /opt/meatybroth
ready=0
for _ in $(seq 1 120); do
  if systemctl is-active --quiet docker &&
     ps_out=$(docker compose --profile public ps --format "{{.Name}} {{.State}}") &&
     echo "$ps_out" | grep -q "meatybroth-web-1 *running" &&
     echo "$ps_out" | grep -q "meatybroth-collector-1 *running" &&
     echo "$ps_out" | grep -q "meatybroth-caddy-1 *running" &&
     status_page=$(curl -fsS http://127.0.0.1:8081/status) &&
     count=$(echo "$status_page" | grep -o "[0-9]* eligible posts" | grep -o "[0-9]*") &&
     test "${count:-0}" -ge 1; then
    ready=1
    break
  fi
  sleep 5
done
test "$ready" = 1
systemctl is-active docker
echo "$ps_out"
echo "reader http 200"
echo "collection status: $count eligible posts"
'
cid=$(aws ssm send-command --region "$region" \
  --instance-ids "$instance" --document-name "AWS-RunShellScript" \
  --comment "meatybroth health" \
  --parameters "{\"commands\":[\"echo $(printf '%s' "$remote" | base64 -w0) | base64 -d | bash\"]}" \
  --output json | python3 -c "import json,sys; print(json.load(sys.stdin)['Command']['CommandId'])")
command_status=""
for _ in $(seq 1 180); do
  command_status=$(aws ssm get-command-invocation --region "$region" --command-id "$cid" \
    --instance-id "$instance" --query Status --output text 2>/dev/null) || command_status=""
  case "$command_status" in
    Success|Failed|Cancelled|TimedOut) break ;;
    Pending|InProgress|Delayed|"") sleep 5 ;;
    *) echo "FAIL: unexpected SSM command status $command_status" >&2; exit 1 ;;
  esac
done
[[ -n "$command_status" ]] || { echo "FAIL: SSM command did not start" >&2; exit 1; }
aws ssm get-command-invocation --region "$region" --command-id "$cid" --instance-id "$instance" \
  --query "[Status, ResponseCode, StandardOutputContent, StandardErrorContent]" --output json | python3 -c "
import json, sys
status, code, output, error = json.load(sys.stdin)
assert status == 'Success', f'SSM command status {status}:\n{error}'
assert str(code) == '0', f'remote checks failed (rc {code}):\nstdout:\n{output}\nstderr:\n{error}'
print(output)
print('remote checks passed: docker active, web+collector+caddy running, reader HTTP 200, status page shows a real eligible-post count')
"

site=$(aws cloudformation describe-stacks --region "$region" --stack-name "$stack" \
  --query "Stacks[0].Outputs[?OutputKey=='SiteUrl'].OutputValue" --output text)
[[ "$site" =~ ^https:// ]] || { echo "FAIL: no HTTPS SiteUrl output (got: '$site')" >&2; exit 1; }
step "public HTTPS reader + collection status: $site"
public_count=""
for _ in $(seq 1 60); do
  public_status=$(curl -fsS --max-time 10 "$site/status" 2>/dev/null) || public_status=""
  public_count=$(printf '%s' "$public_status" | grep -oE '[0-9]+ eligible posts' | head -1 | grep -oE '[0-9]+' || true)
  [[ "${public_count:-0}" =~ ^[1-9][0-9]*$ ]] && break
  sleep 5
done
[[ "${public_count:-0}" =~ ^[1-9][0-9]*$ ]] \
  || { echo "FAIL: public HTTPS status page unavailable or has no eligible-post count" >&2; exit 1; }
echo "deployment healthy: $site/ (public status: $public_count eligible posts)"
echo "SSM debug tunnel: aws ssm start-session --region $region --target $instance --document-name AWS-StartPortForwardingSession --parameters '{\"portNumber\":[\"8081\"],\"localPortNumber\":[\"8081\"]}'"
