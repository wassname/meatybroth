"""Infrastructure config tests: isolation, volume lifecycle, no old targets."""

import os
import subprocess
from pathlib import Path

import pytest
import yaml

ROOT = Path(__file__).resolve().parents[1]
TEMPLATE = ROOT / "infrastructure/cloudformation/meatybroth.yaml"
HEAD_SHA = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True,
                          cwd=ROOT).stdout.strip()


def cf_load(text):
    """Safe YAML load that turns CloudFormation short-form tags into mappings."""
    loader = yaml.SafeLoader
    intrinsics = {"Ref", "Sub", "GetAtt", "Select", "GetAZs", "Join",
                  "Split", "ImportValue", "Base64", "FindInMap", "If"}

    def construct(_loader, suffix, node):
        assert suffix in intrinsics, f"unexpected tag !{suffix}"
        if suffix in ("Ref", "Base64", "GetAZs", "ImportValue"):
            return {f"Fn::{suffix}" if suffix != "Ref" else "Ref":
                    _loader.construct_scalar(node)}
        if suffix == "Sub":
            value = (_loader.construct_scalar(node) if isinstance(node, yaml.ScalarNode)
                     else _loader.construct_sequence(node, deep=True))
            return {"Fn::Sub": value}
        return {f"Fn::{suffix}": _loader.construct_sequence(node, deep=True)}

    loader.add_multi_constructor("!", construct)
    return yaml.load(text, Loader=loader)
COMPOSE = ROOT / "docker-compose.yml"
CADDYFILE = ROOT / "Caddyfile"
DOCKERFILE = ROOT / "Dockerfile"
JUSTFILE = ROOT / "justfile"
README = ROOT / "README.md"

# Inherited Rusty Claw targets that must not survive cleanup.
FORBIDDEN = [
    "therustyclaw.com",
    "therustyclaw-archive",
    "agent-nostr-relay",
    "strfry",
    "Dockerfile.search",
    "Dockerfile.strfry",
]
FORBIDDEN_RECIPE_WORDS = ["aws", "tf-", "deploy-live", "ssh-live"]


@pytest.fixture(scope="module")
def template():
    return cf_load(TEMPLATE.read_text())


@pytest.fixture(scope="module")
def resources(template):
    return template["Resources"]


def resource_types(resources):
    return [props["Type"] for props in resources.values()]


def test_forbidden_strings_absent():
    files = [TEMPLATE, COMPOSE, CADDYFILE, DOCKERFILE, JUSTFILE, README,
             ROOT / "scripts/deploy_cloudformation.sh"]
    for path in files:
        assert path.exists(), path
        text = path.read_text().lower()
        for needle in FORBIDDEN:
            assert needle.lower() not in text, f"{needle} in {path.name}"


def test_old_operational_paths_removed():
    for rel in ["terraform", "plugins", "Dockerfile.search", "Dockerfile.strfry",
                "strfry.conf", "nginx.conf", "skill.md", "docs/api.md",
                "docs/OPERATIONS.md",
                "scripts/rustyclaw_mailbox.py", "scripts/rustyclaw_post.py",
                "scripts/rustyclaw_wait.py"]:
        assert not (ROOT / rel).exists(), rel


def test_public_stack_has_only_needed_edge_resources(resources):
    forbidden_types = {
        "AWS::EC2::NatGateway",
        "AWS::ElasticLoadBalancingV2::LoadBalancer",
        "AWS::ElasticLoadBalancingV2::TargetGroup",
        "AWS::S3::Bucket",
    }
    assert not forbidden_types & set(resource_types(resources))
    assert resource_types(resources).count("AWS::EC2::EIP") == 1
    records = [p for p in resources.values() if p["Type"] == "AWS::Route53::RecordSet"]
    assert len(records) == 1
    record = records[0]["Properties"]
    assert record["Type"] == "A"
    assert record["HostedZoneId"] == {"Ref": "HostedZoneId"}
    assert record["Name"] == {"Ref": "DomainName"}
    assert record["ResourceRecords"] == [{"Ref": "ReaderAddress"}]


def test_security_group_has_https_without_ssh(resources):
    sgs = [p for p in resources.values() if p["Type"] == "AWS::EC2::SecurityGroup"]
    assert len(sgs) == 1
    props = sgs[0]["Properties"]
    ingress = props["SecurityGroupIngress"]
    assert {(rule["IpProtocol"], rule["FromPort"], rule["ToPort"], rule["CidrIp"])
            for rule in ingress} == {
                ("tcp", 80, 80, "0.0.0.0/0"),
                ("tcp", 443, 443, "0.0.0.0/0"),
            }
    assert props["SecurityGroupEgress"], "outbound access required for relays/SSM"


def test_encrypted_volume_deleted_with_instance(resources):
    instances = [p for p in resources.values() if p["Type"] == "AWS::EC2::Instance"]
    assert len(instances) == 1
    ebs = instances[0]["Properties"]["BlockDeviceMappings"][0]["Ebs"]
    assert ebs["Encrypted"] is True
    assert ebs["DeleteOnTermination"] is True
    assert ebs["VolumeType"] == "gp3"


def test_imdsv2_required_and_ssm_only_role(resources):
    instance = next(p for p in resources.values() if p["Type"] == "AWS::EC2::Instance")
    assert instance["Properties"]["MetadataOptions"]["HttpTokens"] == "required"
    role = next(p for p in resources.values() if p["Type"] == "AWS::IAM::Role")
    policies = role["Properties"]["ManagedPolicyArns"]
    assert policies == ["arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"]


def test_pinned_source_revision_required(template):
    param = template["Parameters"]["SourceRevision"]
    assert param["AllowedPattern"] == "^[0-9a-f]{40}$"


def test_userdata_checks_out_pinned_revision(resources):
    instance = next(p for p in resources.values() if p["Type"] == "AWS::EC2::Instance")
    user_data = instance["Properties"]["UserData"]["Fn::Base64"]["Fn::Sub"][0]
    assert "${SourceRevision}" in user_data  # pinned commit, not a branch
    assert "| bash" not in user_data  # never pipe a remote installer into a shell
    assert "| sha256sum -c -" in user_data  # remote downloads must be checksum-pinned
    assert "/opt/meatybroth" in user_data
    assert "MEATYBROTH_DOMAIN=${DomainName}" in user_data
    assert "docker compose --profile public up" in user_data


def test_userdata_matches_verified_al2023_bootstrap(resources):
    """AL2023 lacks Compose and its Buildx is too old for pinned Compose."""
    instance = next(p for p in resources.values() if p["Type"] == "AWS::EC2::Instance")
    props = instance["Properties"]
    user_data = props["UserData"]["Fn::Base64"]["Fn::Sub"][0]
    assert "dnf install -y docker git" in user_data
    assert "dnf install -y docker git curl" not in user_data
    assert "docker-compose-plugin" not in user_data
    assert "v5.5.1/docker-compose-linux-x86_64" in user_data
    assert "db1889184726840f75c4f9c001048430d4f25b3be3cb084d3ddd762bc0aed576" in user_data
    assert "v0.36.1/buildx-v0.36.1.linux-amd64" in user_data
    assert "48af8a397ebd60178778bf63611dbcebe5f5e7a9be90eb9147b24b9587455778" in user_data
    assert set(instance["DependsOn"]) == {"DefaultRoute", "SubnetRouteAssociation"}


def test_compose_keeps_app_private_and_profiles_public_https():
    compose = yaml.safe_load(COMPOSE.read_text())
    services = compose["services"]
    assert set(services) == {"web", "collector", "caddy"}
    assert services["web"]["ports"] == ["127.0.0.1:8081:8081"]
    assert "--loop" in services["collector"]["command"]
    assert services["web"]["volumes"] == ["meatybroth-data:/var/lib/meatybroth"]
    assert services["collector"]["volumes"] == ["meatybroth-data:/var/lib/meatybroth"]
    caddy = services["caddy"]
    assert caddy["profiles"] == ["public"]
    assert caddy["ports"] == ["80:80", "443:443"]
    assert caddy["image"].startswith("caddy@sha256:")
    assert CADDYFILE.read_text() == "{$MEATYBROTH_DOMAIN} {\n\treverse_proxy web:8081\n}\n"


def test_compose_bounded_logs_and_shared_root():
    compose = yaml.safe_load(COMPOSE.read_text())
    for name, svc in compose["services"].items():
        logging = svc["logging"]
        assert logging["driver"] == "json-file"
        assert logging["options"]["max-size"] == "10m" and logging["options"]["max-file"] == "3", name
    for name in ("web", "collector"):
        assert "MEATYBROTH_ROOT" in compose["services"][name]["environment"]


def test_deploy_script_distinguishes_missing_stack_from_auth_failure(tmp_path):
    """Create-only: existing stack refuses; ValidationError-does-not-exist proceeds;
    auth/network errors refuse instead of being read as 'missing'."""
    import os
    import subprocess

    script = ROOT / "scripts/deploy_cloudformation.sh"
    tmp = Path(tmp_path)

    def make_stub(behavior):
        if behavior == "exists":
            describe = '''echo '{"Stacks":[{"StackName":"x"}]}' '''
        elif behavior == "auth":
            describe = '''echo 'An error occurred (AccessDenied) when calling DescribeStacks' >&2; exit 254'''
        else:  # missing
            describe = '''echo 'An error occurred (ValidationError): Stack does not exist' >&2; exit 254'''
        stub = tmp / "aws"
        stub.write_text("#!/bin/bash\n"
                        "printf '%s\\n' \"$*\" >> $TMPDIR_CALLS\n"
                        "case \"$*\" in\n"
                        "  *describe-stacks*) " + describe + " ;;\n"
                        "  *get-caller-identity*) echo 123456789012 ;;\n"
                        "  *list-hosted-zones-by-name*) echo /hostedzone/Z123 ;;\n"
                        "  *describe-change-set*) echo CREATE_COMPLETE ;;\n"
                        "  *create-change-set*|*execute-change-set*|*wait*) echo ok ;;\n"
                        "esac\n")
        stub.chmod(0o755)

    gitstub = tmp / "git"
    gitstub.write_text("#!/bin/bash\ncase \"$*\" in\n  *fetch*|*diff*|*cat-file*) exit 0 ;;\n  *merge-base*) exit 0 ;;\n"
                       "  *remote*get-url*) echo https://github.com/wassname/meatybroth; exit 0 ;;\n"
                       "  *) exec /usr/bin/git \"$@\" ;;\nesac\n")
    gitstub.chmod(0o755)
    curlstub = tmp / "curl"
    curlstub.write_text("#!/bin/bash\nexit 0\n")
    curlstub.chmod(0o755)
    env = {**os.environ, "PATH": f"{tmp}:{os.environ['PATH']}", "TMPDIR_CALLS": str(tmp / "calls")}
    base = [str(script), "--account", "123456789012", "--region", "us-east-1",
            "--stack-name", "meatybroth-exp-test", "--domain", "meatybroth.com",
            "--source-revision", HEAD_SHA]

    make_stub("exists")
    rc = subprocess.run(base, env=env, capture_output=True, text=True)
    assert rc.returncode != 0 and "already exists" in rc.stderr

    make_stub("auth")
    rc = subprocess.run(base, env=env, capture_output=True, text=True)
    assert rc.returncode != 0 and "could not confirm stack absence" in rc.stderr

    make_stub("missing")
    rc = subprocess.run(base, env=env, capture_output=True, text=True)
    assert rc.returncode == 0 and "change set created" in rc.stdout


def test_dockerfile_pinned_and_no_pipe_installs():
    text = DOCKERFILE.read_text()
    assert text.startswith("#")  # header comment
    assert "FROM python@sha256:" in text
    assert "uv sync --frozen" in text
    assert "curl" not in text


def test_justfile_has_isolated_deployment_recipes():
    text = JUSTFILE.read_text()
    recipes = {line.split(":", 1)[0].strip() for line in text.splitlines()
               if line and not line.startswith((" ", "\t", "#")) and ":" in line}
    assert {"setup", "test", "serve", "collect", "smoke", "validate",
            "deploy", "deploy-prepare", "deploy-health"} <= recipes
    for word in FORBIDDEN_RECIPE_WORDS:
        assert word not in recipes


def test_deploy_script_checks_origin_and_executes(tmp_path):
    """--execute runs execute-change-set after an origin-ancestry check; without
    it the script stops at the change set. Agents never push: fetch is read-only."""
    import subprocess, os
    tmp = Path(tmp_path)
    tmpsys = tmp / "bin"; tmpsys.mkdir()
    stub_git = tmpsys / "git"
    def write_git(allow_ancestor):
        stub_git.write_text("#!/bin/bash\ncase \"$*\" in\n"
                            "  *fetch*|*diff*|*cat-file*) exit 0 ;;\n"
                            "  *merge-base*) exit " + ("0" if allow_ancestor else "1") + " ;;\n"
                            "  *remote*get-url*) echo https://github.com/wassname/meatybroth; exit 0 ;;\n"
                            "  *) exec /usr/bin/git \"$@\" ;;\nesac\n")
    write_git(False)
    stub_aws = tmpsys / "aws"
    stub_aws.write_text("#!/bin/bash\nprintf '%s\\n' \"$*\" >> $MOCKLOG\n"
                        "n=$(cat $MOCKLOG.count 2>/dev/null || echo 0)\n"
                        "case \"$*\" in\n"
                        "  *get-caller-identity*) echo 123456789012 ;;\n"
                        "  *list-hosted-zones-by-name*) echo ${MOCK_ZONE:-/hostedzone/Z123} ;;\n"
                        "  *describe-stacks*) "
                        "if [ \"$(cat $MOCKLOG.executed 2>/dev/null)\" = 1 ]; then "
                        "echo \"CREATE_COMPLETE\\tInstanceId=i-123\"; else "
                        "echo 'err (ValidationError): does not exist' >&2; exit 254; fi ;;\n"
                        "  *execute-change-set*) echo 1 > $MOCKLOG.executed; echo ok ;;\n"
                        "  *describe-change-set*) echo CREATE_COMPLETE ;;\n"
                        "  *create-change-set*|*wait*) echo ok ;;\n"
                        "esac\n"
                        "echo $((n+1)) > $MOCKLOG.count\n")
    stub_curl = tmpsys / "curl"
    stub_curl.write_text('#!/bin/bash\ntest "${MOCK_PUBLIC:-1}" = 1\n')
    for p in (stub_git, stub_aws, stub_curl):
        p.chmod(0o755)

    # revision not on origin -> refuse before any AWS mutation
    log = tmp / "calls"; log.unlink(missing_ok=True)
    env = {**os.environ, "PATH": f"{tmpsys}:{os.environ['PATH']}", "MOCKLOG": str(log)}
    base = [str(ROOT / "scripts/deploy_cloudformation.sh"), "--account", "123456789012",
            "--region", "us-east-1", "--stack-name", "meatybroth-exp-test",
            "--domain", "meatybroth.com", "--source-revision", HEAD_SHA]
    rc = subprocess.run(base, env=env, capture_output=True, text=True)
    assert rc.returncode != 0 and "not on origin/main" in rc.stderr
    assert log is None or not log.exists() or "create-change-set" not in log.read_text()

    # A private revision cannot be cloned by EC2 and refuses before AWS use.
    write_git(True)
    log.write_text("")
    rc = subprocess.run(base, env={**env, "MOCK_PUBLIC": "0"}, capture_output=True, text=True)
    assert rc.returncode != 0 and "not publicly readable" in rc.stderr
    assert not log.read_text()

    # A missing public zone refuses before CloudFormation mutation.
    rc = subprocess.run(base, env={**env, "MOCK_ZONE": "None"}, capture_output=True, text=True)
    assert rc.returncode != 0 and "no public Route53 hosted zone" in rc.stderr
    assert "create-change-set" not in log.read_text()

    # On origin with a zone, no --execute stops after change-set creation.
    log.write_text("")
    rc = subprocess.run(base, env=env, capture_output=True, text=True)
    assert rc.returncode == 0 and "NOT executed" in rc.stdout
    assert "execute-change-set" not in log.read_text()

    # --execute: real resources path
    rc = subprocess.run(base + ["--execute"], env=env, capture_output=True, text=True)
    assert rc.returncode == 0 and "change set executed" in rc.stdout
    calls = log.read_text()
    assert "execute-change-set" in calls
    assert "ParameterKey=DomainName,ParameterValue=meatybroth.com" in calls
    assert "ParameterKey=HostedZoneId,ParameterValue=Z123" in calls


def test_deploy_health_checks_app_not_just_stack(tmp_path):
    """Health script checks instance status, SSM command and the served app."""
    import subprocess, os
    tmp = Path(tmp_path)
    tmpsys = tmp / "bin"; tmpsys.mkdir()
    stub_aws = tmpsys / "aws"
    stub_aws.write_text('#!/bin/bash\ncase "$*" in\n'
                        '  *get-caller-identity*) echo 1 ;;\n'
                        '  *describe-stacks*StackStatus*) echo CREATE_COMPLETE ;;\n'
                        '  *describe-stacks*InstanceId*) echo i-04d3f2a1b2c3 ;;\n'
                        '  *describe-stacks*SiteUrl*) echo https://meatybroth.com ;;\n'
                        "  *describe-instance-status*) printf 'running\\tok\\n' ;;\n"
                        '  *describe-instance-information*) echo Online ;;\n'
                        '  *send-command*) echo \'{"Command":{"CommandId":"CID"}}\' ;;\n'
                        '  *wait*) exit 0 ;;\n'
                        '  *get-command-invocation*--query\\ \\[Status,*) echo \'["Success", "0", "docker active\\nmeatybroth-web-1 running\\nmeatybroth-collector-1 running\\nmeatybroth-caddy-1 running\\nreader http 200\\ncollection status: 42 eligible posts", ""]\' ;;\n'
                        '  *get-command-invocation*--query\\ Status*) echo Success ;;\n'
                        'esac\n')
    stub_curl = tmpsys / "curl"
    stub_curl.write_text("#!/bin/bash\necho '42 eligible posts'\n")
    for path in (stub_aws, stub_curl):
        path.chmod(0o755)
    env = {**os.environ, "PATH": f"{tmpsys}:{os.environ['PATH']}"}
    rc = subprocess.run([str(ROOT / "scripts/deploy_health.sh"), "account=1", "region=us-east-1",
                         "stack=meatybroth-exp-test"], env=env, capture_output=True, text=True)
    assert rc.returncode == 0
    assert "instance i-04d3f2a1b2c3" in rc.stdout and "SSM online" in rc.stdout
    assert "remote checks passed" in rc.stdout
    assert "deployment healthy: https://meatybroth.com/ (public status: 42 eligible posts)" in rc.stdout
    assert "start-session" in rc.stdout  # private diagnostic path remains available
    health_script = (ROOT / "scripts/deploy_health.sh").read_text()
    assert "ssm wait command-executed" not in health_script
    assert "seq 1 180" in health_script  # longer than the remote ten-minute bootstrap poll


def test_deploy_health_fails_on_bad_remote_state(tmp_path):
    """A non-Success or rc!=0 remote result must fail the health script."""
    import subprocess, os
    tmp = Path(tmp_path)
    tmpsys = tmp / "bin"; tmpsys.mkdir()
    stub_aws = tmpsys / "aws"
    stub_aws.write_text('#!/bin/bash\ncase "$*" in\n'
                        '  *get-caller-identity*) echo 1 ;;\n'
                        '  *describe-stacks*) if [[ "$*" == *StackStatus* ]]; then echo CREATE_COMPLETE; else echo i-1; fi ;;\n'
                        "  *describe-instance-status*) printf 'running\\tok\\n' ;;\n"
                        '  *describe-instance-information*) echo Online ;;\n'
                        '  *send-command*) echo \'{"Command":{"CommandId":"CID"}}\' ;;\n'
                        '  *wait*) exit 0 ;;\n'
                        '  *get-command-invocation*--query\\ \\[Status,*) echo \'["Failed", "1", "", "docker: not active"]\' ;;\n'
                        '  *get-command-invocation*--query\\ Status*) echo Failed ;;\n'
                        'esac\n')
    stub_aws.chmod(0o755)
    env = {**os.environ, "PATH": f"{tmpsys}:{os.environ['PATH']}"}
    rc = subprocess.run([str(ROOT / "scripts/deploy_health.sh"), "account=1", "region=us-east-1",
                         "stack=meatybroth-exp-test"], env=env, capture_output=True, text=True)
    assert rc.returncode != 0 and "SSM command status Failed" in rc.stderr
