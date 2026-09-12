"""AWS CLI text-format regression for scripts/deploy_health.sh."""

import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/deploy_health.sh"


def health_env(tmp_path, public_curl_output):
    bindir = tmp_path / "bin"
    bindir.mkdir()
    aws = bindir / "aws"
    aws.write_text(
        "#!/bin/bash\n"
        "case \"$*\" in\n"
        "  *get-caller-identity*) echo 1 ;;\n"
        "  *describe-stacks*StackStatus*) echo CREATE_COMPLETE ;;\n"
        "  *describe-stacks*InstanceId*) echo i-04d3f2a1b2c3 ;;\n"
        "  *describe-stacks*SiteUrl*) echo https://meatybroth.com ;;\n"
        "  *describe-instance-status*) printf 'running\\tok\\n' ;;\n"
        "  *describe-instance-information*) echo Online ;;\n"
        "  *send-command*) echo '{\"Command\":{\"CommandId\":\"CID\"}}' ;;\n"
        "  *wait*) exit 0 ;;\n"
        "  *get-command-invocation*--query\\ \\[Status,*) echo '[\"Success\", \"0\", \"reader http 200\\ncollection status: 42 eligible posts\", \"\"]' ;;\n"
        "  *get-command-invocation*--query\\ Status*) echo Success ;;\n"
        "esac\n"
    )
    curl = bindir / "curl"
    curl.write_text(f"#!/bin/bash\nprintf '%s\\n' '{public_curl_output}'\n")
    sleep = bindir / "sleep"
    sleep.write_text("#!/bin/bash\nexit 0\n")
    for path in (aws, curl, sleep):
        path.chmod(0o755)
    return {**os.environ, "PATH": f"{bindir}:{os.environ['PATH']}"}


def test_health_parses_tab_separated_instance_tuple_and_public_count(tmp_path):
    """AWS CLI --output text formats [state, health] as `running\tok`, not lines."""
    result = subprocess.run(
        [str(SCRIPT), "account=1", "region=us-east-1", "stack=meatybroth-test"],
        env=health_env(tmp_path, "42 eligible posts"),
        text=True,
        capture_output=True,
    )
    assert result.returncode == 0, result.stderr
    assert "instance i-04d3f2a1b2c3 running + status ok" in result.stdout
    assert "public status: 42 eligible posts" in result.stdout
    script = SCRIPT.read_text()
    assert "IFS=$'\\t' read -r istate ihealth" in script
    assert "cd /opt/meatybroth" in script  # SSM cwd cannot alter Compose .env resolution


def test_health_refuses_public_http_without_collection_count(tmp_path):
    """A public 200 alone is not deployment proof: /status needs a real count."""
    result = subprocess.run(
        [str(SCRIPT), "account=1", "region=us-east-1", "stack=meatybroth-test"],
        env=health_env(tmp_path, "<html>HTTP 200 but no collection</html>"),
        text=True,
        capture_output=True,
    )
    assert result.returncode != 0
    assert "public HTTPS status page unavailable or has no eligible-post count" in result.stderr
