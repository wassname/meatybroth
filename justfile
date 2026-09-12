# Local recipes and the explicit CloudFormation deployment entry point.

db := ".local/meatybroth.db"
host := "127.0.0.1"
port := "8081"
root := "60c052cf19fbfb973c1779585df423e3982a3a251fc826d4c76f8063621c5bb6"
export AWS_PROFILE := env_var_or_default("AWS_PROFILE", "default")
AWS_ACCOUNT := env_var_or_default("AWS_ACCOUNT", "")
AWS_REGION := env_var_or_default("AWS_REGION", "us-east-1")
STACK_NAME := env_var_or_default("STACK_NAME", "meatybroth-exp-wassname")
DOMAIN := env_var_or_default("DOMAIN", "meatybroth.com")

default:
    @just --list

# Install declared dependencies from uv.lock and verify the result imports
setup:
    uv sync
    uv run python -c "import flask, loguru, meatybroth; print('environment OK')"

# Run the test suite
test:
    uv run pytest -q

# Serve the reader on the loopback UI port; root key configurable like collect
serve:
    uv run python -m meatybroth serve --db {{db}} --root {{root}} --host {{host}} --port {{port}}

# One bounded collection pass into the local database
collect:
    uv run python -m meatybroth collect --db {{db}} --root {{root}}

# Continuous bounded collection, every 15 minutes
collect-loop:
    uv run python -m meatybroth collect --db {{db}} --root {{root}} --loop

# End-to-end check: infrastructure config tests, then serve KNOWN content
# through the production path on a dedicated free port. A pre-existing reader
# on 8081 must not satisfy this: the port is allocated fresh and the check
# asserts this process serves the seeded marker.
smoke:
    #!/bin/bash
    set -euo pipefail
    uv run pytest -q tests/test_infrastructure.py
    marker="meatybroth-smoke-marker-$$"
    port=$(uv run python -c "
    import socket
    s = socket.socket()
    s.bind(('127.0.0.1', 0))
    print(s.getsockname()[1])
    s.close()
    ")
    seed() {  # seed through the production storage path, not a fixture hack
        uv run python -c "
    from meatybroth.store import Store, Post
    import time
    store = Store('$1')
    now = int(time.time())
    assert store.upsert(Post(source='nostr', source_id='$marker', author_id='0'*64,
                             author_name='smoke', text='smoke marker $marker',
                             created_at=now - 60, url='https://example.com/smoke'), now=now)
    "
    }
    # POSITIVE: known content through the production path on a dedicated port.
    db=".local/smoke-pos-$$.db"
    seed "$db"
    uv run python -m meatybroth serve --db "$db" --host 127.0.0.1 --port "$port" &
    pid=$!
    trap 'kill $pid 2>/dev/null || true; rm -f "$db" .local/smoke-neg-$$.db' EXIT
    ok=0
    for _ in $(seq 60); do
        # The canonical-ID article, not echoed query input, proves a stored hit.
        if curl -fsS "http://127.0.0.1:$port/?q=$marker" 2>/dev/null | grep -q "id=\"nostr:$marker\""; then
            ok=1; break
        fi
        kill -0 $pid 2>/dev/null || { echo "smoke: serve process died" >&2; exit 1; }
        sleep 0.5
    done
    [ "$ok" = 1 ] || { echo "smoke: seeded marker article not served within timeout" >&2; exit 1; }
    kill $pid; wait $pid 2>/dev/null || true
    # NEGATIVE CONTROL: an empty corpus must NOT show the marker article.
    # First require the server to be healthy (explicit HTTP 200), then assert
    # marker absence — a timeout or HTTP 500 must fail the smoke, not pass it.
    uv run python -m meatybroth serve --db ".local/smoke-neg-$$.db" --host 127.0.0.1 --port "$port" &
    pid=$!
    up=0
    for _ in $(seq 60); do
        if curl -fsS "http://127.0.0.1:$port/" >/dev/null 2>&1; then up=1; break; fi
        kill -0 $pid 2>/dev/null || { echo "smoke: negative-control serve died" >&2; exit 1; }
        sleep 0.5
    done
    [ "$up" = 1 ] || { echo "smoke: FAIL - negative-control server never returned HTTP 200" >&2; exit 1; }
    # Capture the body first: a failed/500 query request must fail the smoke
    # (errexit), not be read as marker absence.
    body=$(curl -fsS "http://127.0.0.1:$port/?q=$marker")
    echo "$body" | grep -q "id=\"nostr:$marker\"" && {
        echo "smoke: FAIL - marker article served from an empty corpus (reflected input)" >&2; exit 1
    }
    kill -0 $pid 2>/dev/null
    echo "smoke: positive (marker article served on :$port) and negative control (server HTTP 200, empty corpus has no marker article) both passed"

# Live-reload dev server: browser auto-refreshes on Python/template/CSS edits.
dev:
    uv run python scripts/dev_server.py

# First deployment: authenticate, publish the reviewed commit, then `just deploy`.
# Real resources cost about $22/month in us-east-1, plus the existing domain.
deploy:
    @test -n "{{AWS_ACCOUNT}}" || { echo "AWS_ACCOUNT is required" >&2; exit 2; }
    scripts/deploy_cloudformation.sh --account {{AWS_ACCOUNT}} --region {{AWS_REGION}} --stack-name {{STACK_NAME}} --domain {{DOMAIN}} --source-revision "$(git rev-parse HEAD)" --execute
    just deploy-health

# Create and inspect the change set without executing it.
deploy-prepare:
    @test -n "{{AWS_ACCOUNT}}" || { echo "AWS_ACCOUNT is required" >&2; exit 2; }
    scripts/deploy_cloudformation.sh --account {{AWS_ACCOUNT}} --region {{AWS_REGION}} --stack-name {{STACK_NAME}} --domain {{DOMAIN}} --source-revision "$(git rev-parse HEAD)"

# Check the EC2 services and the public HTTPS page.
deploy-health:
    @test -n "{{AWS_ACCOUNT}}" || { echo "AWS_ACCOUNT is required" >&2; exit 2; }
    scripts/deploy_health.sh account={{AWS_ACCOUNT}} region={{AWS_REGION}} stack={{STACK_NAME}}

# Validate every recipe parses without running it
validate:
    just --dry-run --justfile {{justfile()}}
