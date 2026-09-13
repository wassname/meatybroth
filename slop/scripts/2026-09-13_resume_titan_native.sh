#!/usr/bin/env bash
set -euo pipefail
set +x
cd "$(dirname "$0")/../.."

for segment in $(seq 1 20); do
  echo "segment=$segment start=$(date -Is)"
  credentials="$(AWS_REGION=us-east-2 aws configure export-credentials --profile cds-login --format env)"
  eval "$credentials"
  unset credentials AWS_PROFILE
  export AWS_REGION=us-west-2 AWS_EC2_METADATA_DISABLED=true

  segment_log=".local/titan-segment-${segment}.log"
  succeeded=0
  for attempt in 1 2; do
    if MEATYBROTH_DB="$PWD/.local/rust-live-8086/events.sqlite" \
      MEATYBROTH_EMBED_ONLY=1 \
      MEATYBROTH_EMBED_BACKEND=bedrock \
      MEATYBROTH_EMBED_MODEL=amazon.titan-embed-text-v2:0 \
      MEATYBROTH_EMBED_DIMENSIONS=512 \
      MEATYBROTH_EMBED_NORMALIZE=true \
      MEATYBROTH_EMBED_TOTAL_BUDGET_USD=5 \
      MEATYBROTH_EMBED_MONTHLY_BUDGET_USD=5 \
      MEATYBROTH_EMBED_MAX_POSTS=1000 \
      target/release/meatybroth 2>&1 | tee "$segment_log"; then
      succeeded=1
      break
    fi
    echo "segment=$segment attempt=$attempt failed; refreshing credentials once"
    credentials="$(AWS_REGION=us-east-2 aws configure export-credentials --profile cds-login --format env)"
    eval "$credentials"
    unset credentials AWS_PROFILE
    export AWS_REGION=us-west-2 AWS_EC2_METADATA_DISABLED=true
  done
  unset AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AWS_SESSION_TOKEN AWS_CREDENTIAL_EXPIRATION
  if [[ "$succeeded" != 1 ]]; then
    echo "segment=$segment failed twice; stopping"
    exit 1
  fi
  vectors="$(sqlite3 .local/rust-live-8086/events.sqlite "SELECT count(*) FROM post_embeddings WHERE space_id=(SELECT id FROM embedding_spaces WHERE backend='bedrock' ORDER BY created_at DESC LIMIT 1);")"
  cost="$(sqlite3 .local/rust-live-8086/events.sqlite "SELECT printf('%.9f',coalesce(sum(actual_nusd),0)/1000000000.0) FROM embedding_requests WHERE status='succeeded' AND space_id=(SELECT id FROM embedding_spaces WHERE backend='bedrock' ORDER BY created_at DESC LIMIT 1);")"
  echo "segment=$segment finish=$(date -Is) vectors=$vectors actual_cost_usd=$cost"
  if grep -q 'Embedded 0 posts in this batch' "$segment_log"; then
    echo "Titan pending set exhausted"
    exit 0
  fi
done

echo "Titan did not exhaust pending work within 20 bounded segments"
exit 1
