import json
import os
import shutil
import subprocess
import time
import uuid

import pytest

import conftest
from e2e.blackbox import last_assistant_message, message_text
from e2e import scenarios as e2e
from e2e.stack import E2EStack
from talon_client import GetSessionRequest, ListResourcesRequest


@pytest.fixture
def stack(sqlite_local_stack: E2EStack) -> E2EStack:
    return sqlite_local_stack


def acp_vars(
    suffix: str,
    source_ns: str,
    target_ns: str,
    deployment_name: str,
    **extra: str,
) -> dict[str, str]:
    values: dict[str, str] = {
        "run_id": suffix,
        "source_ns": source_ns,
        "target_ns": target_ns,
        "deployment_name": deployment_name,
    }
    values.update(extra)
    return values


def apply_acp_stack(
    cli,
    manifest_names: list[str],
    variables: dict[str, str],
) -> None:
    manifest_dir = e2e.MANIFEST_ROOT / "acp"
    for name in manifest_names:
        e2e.apply(cli, manifest_dir / name, variables)


def list_resources(stub, namespace, kind):
    return list(stub.resources.List(ListResourcesRequest(ns=namespace, kind=kind)).resources)


def wait_for_resources(stub, namespace, kind, expected_names):
    expected_names = set(expected_names)
    last_names = set()
    for _ in range(30):
        resources = list_resources(stub, namespace, kind)
        last_names = {resource.metadata.name for resource in resources}
        if expected_names.issubset(last_names):
            return resources
        time.sleep(1)
    raise AssertionError(
        f"Timed out waiting for {kind} resources {sorted(expected_names)} "
        f"in namespace {namespace}; saw {sorted(last_names)}"
    )


def yaml_scalar(value):
    return json.dumps(value)


def yaml_list_snippet(values, indent=10):
    prefix = " " * indent
    if not values:
        return f"{prefix}[]"
    return "\n".join(f"{prefix}- {yaml_scalar(value)}" for value in values)


def yaml_map_snippet(values, indent=10):
    prefix = " " * indent
    if not values:
        return f"{prefix}{{}}"
    return "\n".join(
        f"{prefix}{key}: {yaml_scalar(value)}" for key, value in values.items()
    )


def ensure_docker_acp_image(image):
    if not shutil.which("docker"):
        pytest.skip("docker is not installed")
    inspect = subprocess.run(
        ["docker", "image", "inspect", image],
        text=True,
        capture_output=True,
        check=False,
    )
    if inspect.returncode == 0:
        return
    local_images = {
        "talon-acp-harness:local",
        "talon-codex-acp:local",
        "talon-zed-codex-acp:local",
    }
    if image not in local_images:
        pytest.skip(f"Docker image {image} is not available locally")

    dockerfile = conftest.REPO_ROOT / "dockerfiles" / "codex-acp.Dockerfile"
    build = subprocess.run(
        [
            "docker",
            "build",
            "-f",
            str(dockerfile),
            "-t",
            image,
            str(conftest.REPO_ROOT),
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    assert build.returncode == 0, (
        f"failed to build {image}\nstdout:\n{build.stdout}\nstderr:\n{build.stderr}"
    )


def live_acp_e2e_enabled():
    return (
        os.environ.get("TALON_ACP_DOCKER_E2E") == "1"
        or os.environ.get("TALON_CODEX_DOCKER_E2E") == "1"
    )


def live_acp_harness_config():
    dotenv = conftest.load_repo_dotenv_values()
    harness = os.environ.get("TALON_ACP_E2E_HARNESS", "codex")
    harness = harness.strip().lower().replace("_", "-")
    if harness in {"claude", "claude-code-acp"}:
        harness = "claude-code"
    elif harness in {"open-code", "opencode-acp", "opencode-ai"}:
        harness = "opencode"
    elif harness == "codex-acp":
        harness = "codex"

    def credential(*keys):
        for key in keys:
            value = os.environ.get(key) or dotenv.get(key)
            if value:
                return key, value
        return None, None

    image = os.environ.get("TALON_ACP_E2E_IMAGE")
    if harness == "codex":
        _, value = credential("CODEX_API_KEY", "OPENAI_API_KEY")
        if not value:
            pytest.skip("CODEX_API_KEY or OPENAI_API_KEY is not available")
        return {
            "harness": harness,
            "label": "Codex",
            "command": "codex-acp",
            "args": [],
            "env": {
                "OPENAI_API_KEY": value,
                "CODEX_API_KEY": value,
            },
            "image": image
            or os.environ.get("TALON_CODEX_ACP_IMAGE")
            or "talon-acp-harness:local",
            "marker": "TALON_CODEX_ACP_CODE_OK",
        }
    if harness == "claude-code":
        _, value = credential("ANTHROPIC_API_KEY")
        if not value:
            pytest.skip("ANTHROPIC_API_KEY is not available")
        return {
            "harness": harness,
            "label": "Claude Code",
            "command": "claude-code-acp",
            "args": [],
            "env": {"ANTHROPIC_API_KEY": value},
            "image": image
            or os.environ.get("TALON_CLAUDE_CODE_ACP_IMAGE")
            or "talon-acp-harness:local",
            "marker": "TALON_CLAUDE_CODE_ACP_CODE_OK",
        }
    if harness == "opencode":
        key, value = credential(
            "OPENCODE_API_KEY",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "GOOGLE_AI_API_KEY",
            "GOOGLE_GENERATIVE_AI_API_KEY",
        )
        if not value:
            pytest.skip(
                "OPENCODE_API_KEY or a supported provider API key is not available"
            )
        return {
            "harness": harness,
            "label": "OpenCode",
            "command": "opencode",
            "args": ["acp"],
            "env": {key: value},
            "image": image
            or os.environ.get("TALON_OPENCODE_ACP_IMAGE")
            or "talon-acp-harness:local",
            "marker": "TALON_OPENCODE_ACP_CODE_OK",
        }
    pytest.skip(
        "TALON_ACP_E2E_HARNESS must be one of codex, claude-code, or opencode"
    )


def wait_for_session_reply(stub, namespace, agent, session_id, expected_text, attempts=30):
    last_state = ""
    last_messages = []
    last_assistant_text = ""
    for _ in range(attempts):
        time.sleep(1)
        res = stub.sessions.Get(
            GetSessionRequest(
                agent=agent,
                session_id=session_id,
                ns=namespace,
            )
        )
        last_state = res.state
        last_messages = list(res.messages)
        assistant = last_assistant_message(res.messages)
        if res.state == "IDLE" and assistant is not None:
            assistant_text = message_text(assistant)
            last_assistant_text = assistant_text
            if expected_text in assistant_text:
                return res
    raise AssertionError(
        f"Timed out waiting for ACP reply containing {expected_text!r}; "
        f"state={last_state!r}, messages={len(last_messages)}, "
        f"last_assistant_text={last_assistant_text!r}"
    )


def wait_for_sandbox_process(stub, namespace):
    last_count = 0
    for _ in range(30):
        sandboxes = list_resources(stub, namespace, "Sandbox")
        if sandboxes:
            sandbox_status = sandboxes[0].status.sandbox
            last_count = len(sandbox_status.processes)
            if sandbox_status.processes:
                return sandboxes[0]
        time.sleep(1)
    raise AssertionError(
        f"Timed out waiting for sandbox process in namespace {namespace}; "
        f"last process count={last_count}"
    )


def wait_for_cli_sandbox(cli, namespace, attempts=60):
    last_count = 0
    for _ in range(attempts):
        sandboxes = e2e.list_resources(cli, "Sandbox", namespace)
        last_count = len(sandboxes)
        if sandboxes:
            return sandboxes[0]
        time.sleep(1)
    raise AssertionError(
        f"Timed out waiting for sandbox in namespace {namespace}; "
        f"last sandbox count={last_count}"
    )


def wait_for_cli_sandbox_process(cli, namespace, attempts=30):
    last_count = 0
    for _ in range(attempts):
        sandboxes = e2e.list_resources(cli, "Sandbox", namespace)
        if sandboxes:
            processes = sandboxes[0].get("status", {}).get("processes", [])
            last_count = len(processes)
            if processes:
                return sandboxes[0]
        time.sleep(1)
    raise AssertionError(
        f"Timed out waiting for sandbox process in namespace {namespace}; "
        f"last process count={last_count}"
    )


def test_cli_apply_acp_deployment_starts_session_sqlite_local_socket(
    stack: E2EStack,
) -> None:
    # Verify the ACP deployment templates render correctly, create the expected
    # runtime resources, and can complete a permission-gated coding session.
    cli = stack.cli()
    suffix = uuid.uuid4().hex[:8]
    source_ns = f"customers-{suffix}"
    target_ns = f"{source_ns}:acme"
    deployment_name = f"company-builder-{suffix}"
    variables = acp_vars(
        suffix,
        source_ns,
        target_ns,
        deployment_name,
        mock_acp_command=conftest.get_binary_path("talon_mock_acp"),
    )
    apply_acp_stack(
        cli,
        [
            "sandbox-class.yaml",
            "coding-agent.template.yaml",
            "coding-sandbox-policy.template.yaml",
            "target-namespace.yaml",
            "deployment.yaml",
        ],
        variables,
    )

    agents = e2e.wait_for_resources(cli, "Agent", ["coding"], target_ns)
    policies = e2e.wait_for_resources(cli, "SandboxPolicy", ["coding"], target_ns)
    replicas = e2e.wait_for_resources(
        cli,
        "DeploymentReplica",
        [f"{deployment_name}--{target_ns.replace(':', '-')}"],
        source_ns,
    )

    rendered_agent = next(
        resource for resource in agents if resource["metadata"]["name"] == "coding"
    )
    assert rendered_agent["kind"] == "Agent"
    assert rendered_agent["spec"]["systemPrompt"].strip() == (
        f"You are the coding agent for {target_ns}."
    )
    assert rendered_agent["spec"]["runtime"]["kind"] == "acp"
    assert rendered_agent["spec"]["runtime"]["acp"]["sandboxPolicyRef"] == "coding"
    assert rendered_agent["spec"]["runtime"]["acp"]["command"] == conftest.get_binary_path(
        "talon_mock_acp"
    )
    permission_policy = rendered_agent["spec"]["runtime"]["acp"]["permissionPolicy"]
    assert permission_policy["default"] == "ask"
    assert permission_policy["filesystemWrite"] == "allow"
    assert permission_policy["terminal"] == "allow"

    rendered_policy = next(
        resource for resource in policies if resource["metadata"]["name"] == "coding"
    )
    assert rendered_policy["kind"] == "SandboxPolicy"
    assert rendered_policy["spec"]["maxConcurrent"] == 2
    assert rendered_policy["spec"]["classRef"]["name"] == f"e2e-code-{suffix}"
    assert rendered_policy["spec"]["template"]["workspace"]["mountPath"] == "/workspace"

    replica = replicas[0]
    assert replica["spec"]["targetNamespace"] == target_ns
    assert replica["status"]["phase"] == "Ready"
    assert sorted(replica["status"]["renderedResources"]) == [
        f"{target_ns}/Agent/coding",
        f"{target_ns}/SandboxPolicy/coding",
    ]

    session_id = e2e.session_create(cli, target_ns, "coding")
    assert session_id

    send = subprocess.Popen(
        [
            *cli.base_args(),
            "session",
            "send",
            "--namespace",
            target_ns,
            "--agent",
            "coding",
            session_id,
            "please request-permission write-file read-file terminal",
        ],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    try:
        request_part, _ = e2e.wait_for_session_part(
            cli,
            target_ns,
            "coding",
            session_id,
            "request_permission",
        )
        request_payload = request_part["payload"]
        request = request_payload["request"]
        assert request["protocol"] == "acp"
        assert request["method"] == "permission/request"
        assert request["action"] == "fileEdit"
        request_id = request_payload["requestId"]

        answer = e2e.session_permission_answer(
            cli,
            target_ns,
            "coding",
            session_id,
            request_id,
            option_id="approved",
        )
        assert answer["requestId"] == request_id
        assert answer["outcome"] == "selected"

        stdout, stderr = send.communicate(timeout=30)
        assert send.returncode == 0, (
            f"talon-cli session send failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        )
    except Exception:
        if send.poll() is None:
            send.terminate()
            try:
                send.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                send.kill()
                send.communicate()
        raise

    completed = e2e.wait_for_session_text(
        cli,
        target_ns,
        "coding",
        session_id,
        "mock response file=written by talon-mock-acp terminal=terminal-ok",
    )
    assert completed["state"] == "IDLE"
    assistant = e2e.assistant_messages(completed)[-1]
    assistant_text = e2e.message_text(assistant)
    assert "file=written by talon-mock-acp" in assistant_text
    assert "terminal=terminal-ok" in assistant_text
    permission_results = e2e.session_parts(completed, "permission_result")
    assert len(permission_results) == 1
    assert permission_results[0]["payload"]["requestId"] == request_id
    permission_outcome = permission_results[0]["payload"]["outcome"]["outcome"]
    assert permission_outcome["optionId"] == "approved"

    sandbox = wait_for_cli_sandbox_process(cli, target_ns)
    sandbox_status = sandbox["status"]
    assert sandbox_status["phase"] == "Ready"
    assert sandbox_status["backendId"].startswith("mock:")
    assert "lease" not in sandbox_status
    assert len(sandbox_status["processes"]) == 1
    process = sandbox_status["processes"][0]
    assert process["command"] == "sh"
    assert process["args"] == ["-lc", "printf terminal-ok"]
    assert process["protocol"] == "terminal"
    assert process["phase"] == "Succeeded"


def test_cli_apply_live_acp_deployment_runs_code_in_sandbox_sqlite_local_socket(
    stack: E2EStack,
) -> None:
    # Verify the selected live ACP Docker harness can create its resources, run
    # code inside the sandbox, and return the expected completion signal.
    if not live_acp_e2e_enabled():
        pytest.skip(
            "set TALON_ACP_DOCKER_E2E=1 to run the live ACP harness Docker e2e"
        )
    harness = live_acp_harness_config()
    ensure_docker_acp_image(harness["image"])

    cli = stack.cli()
    suffix = uuid.uuid4().hex[:8]
    harness_slug = harness["harness"].replace("-", "_")
    source_ns = f"{harness['harness']}-customers-{suffix}"
    target_ns = f"{source_ns}:acme"
    deployment_name = f"{harness['harness']}-company-builder-{suffix}"
    workspace_file = f"/workspace/{harness_slug}_e2e.py"
    variables = acp_vars(
        suffix,
        source_ns,
        target_ns,
        deployment_name,
        docker_image=harness["image"],
        acp_command=harness["command"],
        acp_args_yaml=yaml_list_snippet(harness["args"]),
        acp_env_yaml=yaml_map_snippet(harness["env"]),
        acp_harness_label=harness["label"],
    )
    apply_acp_stack(
        cli,
        [
            "docker-sandbox-class.yaml",
            "acp-harness-agent.template.yaml",
            "docker-coding-sandbox-policy.template.yaml",
            "target-namespace.yaml",
            "deployment.yaml",
        ],
        variables,
    )

    agents = e2e.wait_for_resources(cli, "Agent", ["coding"], target_ns)
    policies = e2e.wait_for_resources(cli, "SandboxPolicy", ["coding"], target_ns)
    e2e.wait_for_resources(
        cli,
        "DeploymentReplica",
        [f"{deployment_name}--{target_ns.replace(':', '-')}"],
        source_ns,
    )

    rendered_agent = next(
        resource for resource in agents if resource["metadata"]["name"] == "coding"
    )
    rendered_acp = rendered_agent["spec"]["runtime"]["acp"]
    assert rendered_acp["command"] == harness["command"]
    assert rendered_acp.get("args", []) == harness["args"]
    assert rendered_acp["cwd"] == "/workspace"
    for key, value in harness["env"].items():
        assert rendered_acp["env"][key] == value
    rendered_policy = next(
        resource for resource in policies if resource["metadata"]["name"] == "coding"
    )
    assert rendered_policy["spec"]["classRef"]["name"] == f"e2e-code-{suffix}"
    assert rendered_policy["spec"]["maxConcurrent"] == 1

    session_id = e2e.session_create(cli, target_ns, "coding")
    e2e.session_send(
        cli,
        target_ns,
        "coding",
        session_id,
        (
            f"Create {workspace_file} with a complete Python program that defines "
            "a function named talon_value, asserts talon_value() == 42, and prints exactly "
            f"{harness['marker']}. Then run python3 {workspace_file}. "
            f"End your final answer with {harness['marker']}."
        ),
        timeout=300,
    )

    completed = e2e.wait_for_session_text(
        cli,
        target_ns,
        "coding",
        session_id,
        harness["marker"],
        attempts=240,
    )
    assistant = e2e.assistant_messages(completed)[-1]
    assistant_text = e2e.message_text(assistant)
    assert harness["marker"] in assistant_text

    sandbox = wait_for_cli_sandbox(cli, target_ns)
    sandbox_status = sandbox["status"]
    assert sandbox_status["phase"] == "Ready"
    assert sandbox_status["backendId"].startswith("docker:")
    assert "lease" not in sandbox_status

    container_id = sandbox_status["backendId"].removeprefix("docker:")
    inspect = subprocess.run(
        [
            "docker",
            "exec",
            container_id,
            "sh",
            "-lc",
            f"cat {workspace_file} && printf '\\n---\\n' && python3 {workspace_file}",
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    try:
        assert inspect.returncode == 0, (
            f"failed to inspect Docker sandbox\nstdout:\n{inspect.stdout}\nstderr:\n{inspect.stderr}"
        )
        assert "def talon_value" in inspect.stdout
        assert harness["marker"] in inspect.stdout
        assert inspect.stdout.rstrip().endswith(harness["marker"])
    finally:
        subprocess.run(
            ["docker", "rm", "-f", container_id],
            text=True,
            capture_output=True,
            check=False,
        )
