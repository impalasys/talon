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
from pathlib import Path

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


def render_acp_manifest(tmp_path, name, replacements):
    source = Path(__file__).resolve().parent / "acp" / name
    content = source.read_text()
    for key, value in replacements.items():
        content = content.replace(key, value)
    target = tmp_path / name
    target.write_text(content)
    return target


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
    if image != "talon-codex-acp:local":
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


def test_cli_apply_acp_deployment_starts_session_sqlite_local_socket(
    gateway_channel_sqlite,
    sqlite_test_grpc_port,
    tmp_path,
):
    stub = GatewayServiceStub(gateway_channel_sqlite)
    suffix = uuid.uuid4().hex[:8]
    source_ns = f"customers-{suffix}"
    target_ns = f"{source_ns}:acme"
    deployment_name = f"company-builder-{suffix}"
    replacements = {
        "__RUN_ID__": suffix,
        "__SOURCE_NS__": source_ns,
        "__TARGET_NS__": target_ns,
        "__DEPLOYMENT_NAME__": deployment_name,
        "__MOCK_ACP_COMMAND__": json.dumps(conftest.get_binary_path("talon_mock_acp")),
    }
    manifests = [
        render_acp_manifest(tmp_path, "sandbox-class.yaml", replacements),
        render_acp_manifest(tmp_path, "coding-agent.template.yaml", replacements),
        render_acp_manifest(tmp_path, "coding-sandbox-policy.template.yaml", replacements),
        render_acp_manifest(tmp_path, "target-namespace.yaml", replacements),
        render_acp_manifest(tmp_path, "deployment.yaml", replacements),
    ]

    for manifest in manifests:
        apply_manifest_with_cli(manifest, sqlite_test_grpc_port)

    agents = wait_for_resources(stub, target_ns, "Agent", ["coding"])
    policies = wait_for_resources(stub, target_ns, "SandboxPolicy", ["coding"])
    replicas = wait_for_resources(
        stub,
        source_ns,
        "DeploymentReplica",
        [f"{deployment_name}--{target_ns.replace(':', '-')}"],
    )

    rendered_agent = next(resource for resource in agents if resource.metadata.name == "coding")
    assert rendered_agent.kind == "Agent"
    assert rendered_agent.spec.agent.system_prompt == f"You are the coding agent for {target_ns}.\n"
    assert rendered_agent.spec.agent.runtime.kind == "acp"
    assert rendered_agent.spec.agent.runtime.acp.sandbox_policy_ref == "coding"
    assert rendered_agent.spec.agent.runtime.acp.command == conftest.get_binary_path(
        "talon_mock_acp"
    )

    rendered_policy = next(
        resource for resource in policies if resource.metadata.name == "coding"
    )
    assert rendered_policy.kind == "SandboxPolicy"
    assert rendered_policy.spec.sandbox_policy.max_concurrent == 2
    assert rendered_policy.spec.sandbox_policy.class_ref.name == f"e2e-code-{suffix}"
    assert rendered_policy.spec.sandbox_policy.template.workspace.mount_path == "/workspace"

    replica = replicas[0]
    assert replica.spec.deployment_replica.target_namespace == target_ns
    assert replica.status.deployment_replica.phase == "Ready"
    assert sorted(replica.status.deployment_replica.rendered_resources) == [
        f"{target_ns}/Agent/coding",
        f"{target_ns}/SandboxPolicy/coding",
    ]

    session = stub.CreateSession(CreateSessionRequest(agent="coding", ns=target_ns))
    session_id = session.session_id
    assert session_id

    stub.SendMessage(
        SendMessageRequest(
            agent="coding",
            session_id=session_id,
            ns=target_ns,
            message="please write-file read-file terminal",
        )
    )

    completed = wait_for_session_reply(
        stub,
        target_ns,
        "coding",
        session_id,
        "mock response file=written by talon-mock-acp terminal=terminal-ok",
    )
    assert completed.state == "IDLE"
    assistant = last_assistant_message(completed.messages)
    assert assistant is not None
    assert assistant.role == 2
    assistant_text = message_text(assistant)
    assert "file=written by talon-mock-acp" in assistant_text
    assert "terminal=terminal-ok" in assistant_text

    sandbox = wait_for_sandbox_process(stub, target_ns)
    sandbox_status = sandbox.status.sandbox
    assert sandbox_status.phase == "Ready"
    assert sandbox_status.backend_id.startswith("mock:")
    assert not sandbox_status.HasField("lease")
    assert len(sandbox_status.processes) == 1
    process = sandbox_status.processes[0]
    assert process.command == "sh"
    assert list(process.args) == ["-lc", "printf terminal-ok"]
    assert process.protocol == "terminal"
    assert process.phase == "Succeeded"


def test_cli_apply_codex_acp_deployment_starts_session_sqlite_local_socket(
    gateway_channel_sqlite,
    sqlite_test_grpc_port,
    tmp_path,
):
    if os.environ.get("TALON_CODEX_E2E") != "1":
        pytest.skip("set TALON_CODEX_E2E=1 to run the live Codex ACP e2e")
    codex = shutil.which("codex")
    dotenv = conftest.load_repo_dotenv_values()
    if not codex:
        pytest.skip("codex CLI is not installed")
    if not os.environ.get("OPENAI_API_KEY") and not dotenv.get("OPENAI_API_KEY"):
        pytest.skip("OPENAI_API_KEY is not available in environment or repo .env")

    stub = GatewayServiceStub(gateway_channel_sqlite)
    suffix = uuid.uuid4().hex[:8]
    source_ns = f"codex-customers-{suffix}"
    target_ns = f"{source_ns}:acme"
    deployment_name = f"codex-company-builder-{suffix}"
    replacements = {
        "__RUN_ID__": suffix,
        "__SOURCE_NS__": source_ns,
        "__TARGET_NS__": target_ns,
        "__DEPLOYMENT_NAME__": deployment_name,
        "__CODEX_ACP_COMMAND__": json.dumps(conftest.get_binary_path("talon_codex_acp")),
        "__CODEX_COMMAND__": json.dumps(codex),
    }
    manifests = [
        render_acp_manifest(tmp_path, "sandbox-class.yaml", replacements),
        render_acp_manifest(tmp_path, "codex-agent.template.yaml", replacements),
        render_acp_manifest(tmp_path, "coding-sandbox-policy.template.yaml", replacements),
        render_acp_manifest(tmp_path, "target-namespace.yaml", replacements),
        render_acp_manifest(tmp_path, "deployment.yaml", replacements),
    ]

    for manifest in manifests:
        apply_manifest_with_cli(manifest, sqlite_test_grpc_port)

    agents = wait_for_resources(stub, target_ns, "Agent", ["coding"])
    wait_for_resources(stub, target_ns, "SandboxPolicy", ["coding"])
    wait_for_resources(
        stub,
        source_ns,
        "DeploymentReplica",
        [f"{deployment_name}--{target_ns.replace(':', '-')}"],
    )
    rendered_agent = next(resource for resource in agents if resource.metadata.name == "coding")
    assert rendered_agent.spec.agent.runtime.acp.command == conftest.get_binary_path(
        "talon_codex_acp"
    )
    assert rendered_agent.spec.agent.runtime.acp.env["TALON_CODEX_COMMAND"] == codex

    session = stub.CreateSession(CreateSessionRequest(agent="coding", ns=target_ns))
    session_id = session.session_id
    stub.SendMessage(
        SendMessageRequest(
            agent="coding",
            session_id=session_id,
            ns=target_ns,
            message="Say exactly TALON_CODEX_OK and nothing else.",
        )
    )

    completed = wait_for_session_reply(
        stub,
        target_ns,
        "coding",
        session_id,
        "codex response=TALON_CODEX_OK file=TALON_CODEX_OK terminal=codex-terminal-ok",
        attempts=180,
    )
    assistant = last_assistant_message(completed.messages)
    assert assistant is not None
    assistant_text = message_text(assistant)
    assert "codex response=TALON_CODEX_OK" in assistant_text
    assert "file=TALON_CODEX_OK" in assistant_text
    assert "terminal=codex-terminal-ok" in assistant_text

    sandbox = wait_for_sandbox_process(stub, target_ns)
    sandbox_status = sandbox.status.sandbox
    assert sandbox_status.phase == "Ready"
    assert sandbox_status.backend_id.startswith("mock:")
    assert not sandbox_status.HasField("lease")
    assert len(sandbox_status.processes) == 1
    process = sandbox_status.processes[0]
    assert process.command == "sh"
    assert list(process.args) == ["-lc", "printf codex-terminal-ok"]
    assert process.protocol == "terminal"
    assert process.phase == "Succeeded"


def test_cli_apply_zed_codex_acp_deployment_runs_code_in_sandbox_sqlite_local_socket(
    gateway_channel_sqlite,
    sqlite_test_grpc_port,
    tmp_path,
):
    if os.environ.get("TALON_CODEX_DOCKER_E2E") != "1":
        pytest.skip("set TALON_CODEX_DOCKER_E2E=1 to run the live Zed Codex ACP e2e")
    dotenv = conftest.load_repo_dotenv_values()
    openai_api_key = os.environ.get("OPENAI_API_KEY") or dotenv.get("OPENAI_API_KEY")
    if not openai_api_key:
        pytest.skip("OPENAI_API_KEY is not available in environment or repo .env")

    image = os.environ.get("TALON_CODEX_ACP_IMAGE", "talon-codex-acp:local")
    ensure_docker_codex_image(image)

    stub = GatewayServiceStub(gateway_channel_sqlite)
    suffix = uuid.uuid4().hex[:8]
    source_ns = f"zed-codex-customers-{suffix}"
    target_ns = f"{source_ns}:acme"
    deployment_name = f"zed-codex-company-builder-{suffix}"
    replacements = {
        "__RUN_ID__": suffix,
        "__SOURCE_NS__": source_ns,
        "__TARGET_NS__": target_ns,
        "__DEPLOYMENT_NAME__": deployment_name,
        "__DOCKER_IMAGE__": image,
        "__OPENAI_API_KEY__": json.dumps(openai_api_key),
    }
    manifests = [
        render_acp_manifest(tmp_path, "docker-sandbox-class.yaml", replacements),
        render_acp_manifest(tmp_path, "zed-codex-agent.template.yaml", replacements),
        render_acp_manifest(tmp_path, "docker-coding-sandbox-policy.template.yaml", replacements),
        render_acp_manifest(tmp_path, "target-namespace.yaml", replacements),
        render_acp_manifest(tmp_path, "deployment.yaml", replacements),
    ]

    for manifest in manifests:
        apply_manifest_with_cli(manifest, sqlite_test_grpc_port)

    agents = wait_for_resources(stub, target_ns, "Agent", ["coding"])
    policies = wait_for_resources(stub, target_ns, "SandboxPolicy", ["coding"])
    wait_for_resources(
        stub,
        source_ns,
        "DeploymentReplica",
        [f"{deployment_name}--{target_ns.replace(':', '-')}"],
    )

    rendered_agent = next(resource for resource in agents if resource.metadata.name == "coding")
    assert rendered_agent.spec.agent.runtime.acp.command == "codex-acp"
    assert rendered_agent.spec.agent.runtime.acp.cwd == "/workspace"
    assert rendered_agent.spec.agent.runtime.acp.env["CODEX_API_KEY"] == openai_api_key
    rendered_policy = next(
        resource for resource in policies if resource.metadata.name == "coding"
    )
    assert rendered_policy.spec.sandbox_policy.class_ref.name == f"e2e-code-{suffix}"
    assert rendered_policy.spec.sandbox_policy.max_concurrent == 1

    session = stub.CreateSession(CreateSessionRequest(agent="coding", ns=target_ns))
    session_id = session.session_id
    stub.SendMessage(
        SendMessageRequest(
            agent="coding",
            session_id=session_id,
            ns=target_ns,
            message=(
                "Create /workspace/zed_codex_e2e.py with a complete Python program that defines "
                "a function named talon_value, asserts talon_value() == 42, and prints exactly "
                "TALON_ZED_CODEX_CODE_OK. Then run python3 /workspace/zed_codex_e2e.py. "
                "End your final answer with TALON_ZED_CODEX_CODE_OK."
            ),
        )
    )

    completed = wait_for_session_reply(
        stub,
        target_ns,
        "coding",
        session_id,
        "TALON_ZED_CODEX_CODE_OK",
        attempts=240,
    )
    assistant = last_assistant_message(completed.messages)
    assert assistant is not None
    assistant_text = message_text(assistant)
    assert "TALON_ZED_CODEX_CODE_OK" in assistant_text

    sandbox = wait_for_sandbox(stub, target_ns)
    sandbox_status = sandbox.status.sandbox
    assert sandbox_status.phase == "Ready"
    assert sandbox_status.backend_id.startswith("docker:")
    assert not sandbox_status.HasField("lease")

    container_id = sandbox_status.backend_id.removeprefix("docker:")
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
