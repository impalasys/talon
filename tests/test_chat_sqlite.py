import grpc
import json
import pytest
import sys
import os
import shutil
import subprocess
import threading
import time
import uuid

# Important: Add generated protos to path so "proto.xxx" resolves locally and not to proto_plus
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))

from proto.gateway_pb2_grpc import GatewayServiceStub
from proto.gateway_pb2 import (
    CreateNamespaceRequest,
    CreateResourceRequest,
    CreateSessionRequest,
    GetSessionRequest,
    ListResourcesRequest,
    SendMessageRequest,
    StreamSessionPartsRequest,
)
from proto.resources.agents_pb2 import AgentSpec, Model
from proto.resources.common_pb2 import ResourceMeta
from proto.resources.resource_pb2 import ResourceManifest, ResourceSpec
import conftest
from e2e.cli_harness import TalonCli
from e2e import scenarios as e2e

PART_TYPE_TEXT = 1
PART_TYPE_REASONING = 2
PART_TYPE_USAGE = 5


def message_text(message):
    return "".join(part.content for part in message.parts if part.part_type == PART_TYPE_TEXT)


def assistant_messages(messages):
    return [message for message in messages if message.role == 2]


def last_assistant_message(messages):
    assistants = assistant_messages(messages)
    return assistants[-1] if assistants else None


def ensure_namespace(stub, name):
    try:
        stub.CreateNamespace(CreateNamespaceRequest(name=name, recursive=True))
    except grpc.RpcError as err:
        if err.code() != grpc.StatusCode.ALREADY_EXISTS:
            raise


def create_agent_resource(stub, ns, name, spec):
    return stub.CreateResource(
        CreateResourceRequest(
            ns=ns,
            manifest=ResourceManifest(
                api_version="talon.impalasys.com/v1",
                kind="Agent",
                metadata=ResourceMeta(name=name, namespace=ns),
                spec=ResourceSpec(agent=spec),
            ),
        )
    ).resource


def test_single_turn_chat_sqlite_local_socket(gateway_channel_sqlite, mock_llm_server):
    stub = GatewayServiceStub(gateway_channel_sqlite)

    ensure_namespace(stub, "talon-sqlite-test")

    agent = create_agent_resource(
        stub,
        "talon-sqlite-test",
        "test-llm-agent",
        AgentSpec(
                model_policy={
                    "profiles": [
                        {
                            "name": "default",
                            "model": Model(
                                provider="mock",
                                name="minimax-m2.7",
                                temperature=0.7,
                            ),
                        }
                    ]
                },
                system_prompt="You are a helpful test assistant.",
            ),
    )
    assert agent.metadata.name == "test-llm-agent"

    session = stub.CreateSession(
        CreateSessionRequest(agent="test-llm-agent", ns="talon-sqlite-test")
    )
    session_id = session.session_id
    assert session_id != ""

    stub.SendMessage(
        SendMessageRequest(
            agent="test-llm-agent",
            session_id=session_id,
            ns="talon-sqlite-test",
            message="What is the square root of 144?",
        )
    )

    success = False
    messages = []
    for _ in range(30):
        time.sleep(1)
        res = stub.GetSession(
            GetSessionRequest(
                agent="test-llm-agent",
                session_id=session_id,
                ns="talon-sqlite-test",
            )
        )
        messages = res.messages
        assistant = last_assistant_message(messages)
        if res.state == "IDLE" and assistant is not None:
            success = True
            break

    assert success, "Agent did not reply in time or failed to revert to IDLE"
    agent_message = last_assistant_message(messages)
    assert agent_message is not None
    assert agent_message.role == 2
    assert "12" in message_text(agent_message)


def test_streaming_chat_sqlite_local_socket(gateway_channel_sqlite, mock_llm_server):
    stub = GatewayServiceStub(gateway_channel_sqlite)

    ensure_namespace(stub, "talon-sqlite-stream-test")

    create_agent_resource(
        stub,
        "talon-sqlite-stream-test",
        "stream-agent",
        AgentSpec(
                model_policy={
                    "profiles": [
                        {
                            "name": "default",
                            "model": Model(
                                provider="mock",
                                name="minimax",
                                temperature=0.7,
                            ),
                        }
                    ]
                },
                system_prompt="Stream me.",
            ),
    )

    session = stub.CreateSession(
        CreateSessionRequest(agent="stream-agent", ns="talon-sqlite-stream-test")
    )
    session_id = session.session_id

    def send_msg():
        time.sleep(2.0)
        stub.SendMessage(
            SendMessageRequest(
                agent="stream-agent",
                session_id=session_id,
                ns="talon-sqlite-stream-test",
                message="Stream test message",
            )
        )

    sender = threading.Thread(target=send_msg)
    sender.start()

    stream_req = StreamSessionPartsRequest(
        agent="stream-agent",
        session_id=session_id,
        ns="talon-sqlite-stream-test",
    )
    events = []
    try:
        saw_reasoning = False
        saw_token = False
        saw_usage = False
        for idx, event in enumerate(stub.StreamSessionParts(stream_req)):
            events.append(event)
            if event.part.part_type == PART_TYPE_REASONING:
                saw_reasoning = True
            if event.part.part_type == PART_TYPE_TEXT:
                saw_token = True
            if event.part.part_type == PART_TYPE_USAGE:
                saw_usage = True
            if saw_reasoning and saw_token and saw_usage:
                break
            if idx > 20:
                break
    except grpc.RpcError:
        pass
    sender.join()

    assert len(events) >= 1
    reasoning_events = [event for event in events if event.part.part_type == PART_TYPE_REASONING]
    token_events = [event for event in events if event.part.part_type == PART_TYPE_TEXT]
    usage_events = [event for event in events if event.part.part_type == PART_TYPE_USAGE]
    assert len(reasoning_events) >= 1
    assert len(token_events) >= 1
    assert len(usage_events) >= 1
    assert "Inspecting the request" in reasoning_events[0].part.content
    streamed_text = "".join(event.part.content for event in token_events)
    assert "received" in streamed_text


def apply_manifest_with_cli(path, grpc_port):
    cli = conftest.get_binary_path("talon_cli")
    result = subprocess.run(
        [
            cli,
            "--gateway",
            f"http://127.0.0.1:{grpc_port}",
            "apply",
            "-f",
            str(path),
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0, (
        f"talon-cli apply failed for {path}\n"
        f"stdout:\n{result.stdout}\n"
        f"stderr:\n{result.stderr}"
    )
    return result.stdout


def cli_for_grpc_port(grpc_port):
    return TalonCli(
        conftest.get_binary_path("talon_cli"),
        f"http://127.0.0.1:{grpc_port}",
    )


def acp_vars(suffix, source_ns, target_ns, deployment_name, **extra):
    values = {
        "run_id": suffix,
        "source_ns": source_ns,
        "target_ns": target_ns,
        "deployment_name": deployment_name,
    }
    values.update(extra)
    return values


def apply_acp_stack(cli, manifest_names, variables):
    manifest_dir = e2e.MANIFEST_ROOT / "acp"
    for name in manifest_names:
        e2e.apply(cli, manifest_dir / name, variables)


def test_cli_chat_sqlite_local_socket(sqlite_test_grpc_port, mock_llm_server):
    cli = cli_for_grpc_port(sqlite_test_grpc_port)
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-cli-chat-{suffix}"
    agent = "cli-chat-agent"
    e2e.apply(
        cli,
        e2e.MANIFEST_ROOT / "chat" / "agent.yaml",
        {
            "namespace": namespace,
            "agent": agent,
            "system_prompt": "You are a helpful CLI E2E test assistant.",
        },
    )
    session_id = e2e.session_create(cli, namespace, agent)
    e2e.session_send(
        cli,
        namespace,
        agent,
        session_id,
        "What is the square root of 144?",
    )
    completed = e2e.wait_for_session_text(cli, namespace, agent, session_id, "12")
    assert completed["state"] == "IDLE"


def test_cli_streaming_chat_sqlite_local_socket(sqlite_test_grpc_port, mock_llm_server):
    cli = cli_for_grpc_port(sqlite_test_grpc_port)
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-cli-stream-{suffix}"
    agent = "cli-stream-agent"
    e2e.apply(
        cli,
        e2e.MANIFEST_ROOT / "chat" / "agent.yaml",
        {
            "namespace": namespace,
            "agent": agent,
            "system_prompt": "You are a helpful streaming CLI E2E test assistant.",
        },
    )

    session_id = e2e.session_create(cli, namespace, agent)
    events = e2e.session_send_stream_json(
        cli,
        namespace,
        agent,
        session_id,
        "Stream test message",
    )
    parts = [
        event.get("part") or {}
        for event in events
        if event.get("part") is not None
    ]
    reasoning = [part for part in parts if part.get("type") == "reasoning"]
    text = [part for part in parts if part.get("type") == "text"]
    usage = [part for part in parts if part.get("type") == "usage"]

    assert reasoning
    assert text
    assert usage
    assert "Inspecting the request" in reasoning[0].get("content", "")
    assert "received" in "".join(part.get("content", "") for part in text)


def test_cli_apply_rejects_status_in_resource_manifest(sqlite_test_grpc_port, tmp_path):
    manifest = tmp_path / "agent-with-status.yaml"
    manifest.write_text(
        """
apiVersion: talon.impalasys.com/v1
kind: Agent
metadata:
  name: status-owned
  namespace: status-owned-test
spec:
  systemPrompt: This manifest should fail before apply.
status:
  phase: Ready
"""
    )
    cli = conftest.get_binary_path("talon_cli")
    result = subprocess.run(
        [
            cli,
            "--gateway",
            f"http://127.0.0.1:{sqlite_test_grpc_port}",
            "apply",
            "-f",
            str(manifest),
        ],
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode != 0
    assert "Resource manifests cannot set status" in (result.stderr + result.stdout)


def list_resources(stub, namespace, kind):
    return list(stub.ListResources(ListResourcesRequest(ns=namespace, kind=kind)).resources)


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


def ensure_docker_codex_image(image):
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
    if image != "talon-zed-codex-acp:local":
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


def wait_for_session_reply(stub, namespace, agent, session_id, expected_text, attempts=30):
    last_state = ""
    last_messages = []
    last_assistant_text = ""
    for _ in range(attempts):
        time.sleep(1)
        res = stub.GetSession(
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


def wait_for_sandbox(stub, namespace):
    last_count = 0
    for _ in range(60):
        sandboxes = list_resources(stub, namespace, "Sandbox")
        last_count = len(sandboxes)
        if sandboxes:
            return sandboxes[0]
        time.sleep(1)
    raise AssertionError(
        f"Timed out waiting for sandbox in namespace {namespace}; "
        f"last sandbox count={last_count}"
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
    sqlite_test_grpc_port,
):
    cli = cli_for_grpc_port(sqlite_test_grpc_port)
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


def test_cli_apply_zed_codex_acp_deployment_runs_code_in_sandbox_sqlite_local_socket(
    sqlite_test_grpc_port,
):
    if os.environ.get("TALON_CODEX_DOCKER_E2E") != "1":
        pytest.skip("set TALON_CODEX_DOCKER_E2E=1 to run the live Zed Codex ACP e2e")
    dotenv = conftest.load_repo_dotenv_values()
    openai_api_key = os.environ.get("OPENAI_API_KEY") or dotenv.get("OPENAI_API_KEY")
    if not openai_api_key:
        pytest.skip("OPENAI_API_KEY is not available in environment or repo .env")

    image = os.environ.get("TALON_CODEX_ACP_IMAGE", "talon-zed-codex-acp:local")
    ensure_docker_codex_image(image)

    cli = cli_for_grpc_port(sqlite_test_grpc_port)
    suffix = uuid.uuid4().hex[:8]
    source_ns = f"zed-codex-customers-{suffix}"
    target_ns = f"{source_ns}:acme"
    deployment_name = f"zed-codex-company-builder-{suffix}"
    variables = acp_vars(
        suffix,
        source_ns,
        target_ns,
        deployment_name,
        docker_image=image,
        openai_api_key=openai_api_key,
    )
    apply_acp_stack(
        cli,
        [
            "docker-sandbox-class.yaml",
            "zed-codex-agent.template.yaml",
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
    assert rendered_agent["spec"]["runtime"]["acp"]["command"] == "codex-acp"
    assert rendered_agent["spec"]["runtime"]["acp"]["cwd"] == "/workspace"
    assert rendered_agent["spec"]["runtime"]["acp"]["env"]["CODEX_API_KEY"] == openai_api_key
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
            "Create /workspace/zed_codex_e2e.py with a complete Python program that defines "
            "a function named talon_value, asserts talon_value() == 42, and prints exactly "
            "TALON_ZED_CODEX_CODE_OK. Then run python3 /workspace/zed_codex_e2e.py. "
            "End your final answer with TALON_ZED_CODEX_CODE_OK."
        ),
    )

    completed = e2e.wait_for_session_text(
        cli,
        target_ns,
        "coding",
        session_id,
        "TALON_ZED_CODEX_CODE_OK",
        attempts=240,
    )
    assistant = e2e.assistant_messages(completed)[-1]
    assistant_text = e2e.message_text(assistant)
    assert "TALON_ZED_CODEX_CODE_OK" in assistant_text

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
            "cat /workspace/zed_codex_e2e.py && printf '\\n---\\n' && python3 /workspace/zed_codex_e2e.py",
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
        assert "TALON_ZED_CODEX_CODE_OK" in inspect.stdout
        assert inspect.stdout.rstrip().endswith("TALON_ZED_CODEX_CODE_OK")
    finally:
        subprocess.run(
            ["docker", "rm", "-f", container_id],
            text=True,
            capture_output=True,
            check=False,
        )
