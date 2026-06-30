import asyncio
import os
import subprocess
import sys
import textwrap
import uuid
from urllib.parse import urlparse, urlunparse

import grpc
import httpx
import pytest

# Important: Add generated protos to path so "proto.xxx" resolves locally and not to proto_plus
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))

from a2a.client import A2ACardResolver
from a2a.client.client import ClientConfig
from a2a.client.client_factory import ClientFactory
from a2a.types import Message, Role, TextPart, TransportProtocol
import conftest
from proto.talon.v1 import namespaces_pb2


def card_resolver(client: httpx.AsyncClient, agent_card_url: str) -> A2ACardResolver:
    parsed_url = urlparse(agent_card_url)
    base_url = f"{parsed_url.scheme}://{parsed_url.netloc}"
    path_with_query = urlunparse(("", "", parsed_url.path, "", parsed_url.query, ""))
    card_path = path_with_query.lstrip("/")
    if card_path:
        return A2ACardResolver(client, base_url, agent_card_path=card_path)
    return A2ACardResolver(client, base_url)


def dump_event(event):
    if isinstance(event, tuple):
        event = event[1] if event[1] is not None else event[0]
    if hasattr(event, "model_dump"):
        return event.model_dump(exclude_none=True)
    return event


def artifact_text(event_dump):
    artifact = event_dump.get("artifact") or event_dump.get("artifactUpdate", {}).get("artifact") or {}
    parts = artifact.get("parts") or []
    return "".join(part.get("text", "") for part in parts if isinstance(part, dict))


def artifact_field(event_dump, snake_name, camel_name):
    artifact = event_dump.get("artifact") or event_dump.get("artifactUpdate", {}).get("artifact") or {}
    return get_field(artifact, snake_name, camel_name)


def artifact_texts(task_dump):
    texts = []
    for artifact in task_dump.get("artifacts", []):
        parts = artifact.get("parts") or []
        texts.append("".join(part.get("text", "") for part in parts if isinstance(part, dict)))
    return texts


def task_history_texts(task_dump):
    texts = []
    for message in task_dump.get("history", []):
        parts = message.get("parts") or message.get("content") or []
        texts.append("".join(part.get("text", "") for part in parts if isinstance(part, dict)))
    return texts


def status_message_text(event_dump):
    message = event_dump.get("status", {}).get("message") or {}
    parts = message.get("parts") or message.get("content") or []
    return "".join(part.get("text", "") for part in parts if isinstance(part, dict))


def get_field(value, snake_name, camel_name):
    return value.get(snake_name, value.get(camel_name))


def grpc_web_frame(message):
    payload = message.SerializeToString()
    return b"\x00" + len(payload).to_bytes(4, "big") + payload


def parse_grpc_web_response(response_body, message_type):
    assert len(response_body) >= 5
    assert response_body[0] == 0
    message_len = int.from_bytes(response_body[1:5], "big")
    assert len(response_body) >= 5 + message_len
    message = message_type()
    message.ParseFromString(response_body[5 : 5 + message_len])
    return message


def api_key_access_token(grpc_port: int, api_key: str) -> str:
    raw_channel = grpc.insecure_channel(f"127.0.0.1:{grpc_port}")
    try:
        return conftest.ApiKeyTokenSource(raw_channel, api_key).token()
    finally:
        raw_channel.close()


async def assert_grpc_web_list_namespaces(gateway_url: str, access_token: str):
    async with httpx.AsyncClient(timeout=30.0) as http_client:
        response = await http_client.post(
            f"{gateway_url}/talon.v1.NamespaceService/List",
            headers={
                "authorization": f"Bearer {access_token}",
                "content-type": "application/grpc-web+proto",
                "x-grpc-web": "1",
            },
            content=grpc_web_frame(namespaces_pb2.ListNamespacesRequest()),
        )
    response.raise_for_status()
    assert response.headers["content-type"].startswith("application/grpc-web")
    list_response = parse_grpc_web_response(
        response.content,
        namespaces_pb2.ListNamespacesResponse,
    )
    assert any(namespace.name == "default" for namespace in list_response.namespaces)


def bool_field(value, snake_name, camel_name):
    field_value = get_field(value, snake_name, camel_name)
    return bool(field_value)


def apply_manifest(path, gateway_url, api_key, auth_file):
    env = os.environ.copy()
    for key in (
        "TALON_GATEWAY_TOKEN",
        "GATEWAY_TOKEN",
        "TALON_GATEWAY_PASSWORD",
        "GATEWAY_PASSWORD",
    ):
        env.pop(key, None)
    env["TALON_API_KEY"] = api_key
    env["TALON_AUTH_FILE"] = auth_file
    subprocess.run(
        [
            conftest.get_binary_path("talon_cli"),
            "--gateway",
            gateway_url,
            "apply",
            "--file",
            str(path),
        ],
        check=True,
        env=env,
    )


def write_manifest(tmp_path, name, content):
    path = tmp_path / name
    path.write_text(textwrap.dedent(content).strip() + "\n")
    return path


def create_a2a_fixture(
    namespace: str,
    agent_name: str,
    tmp_path,
    gateway_url: str,
    api_key: str,
    auth_file: str,
):
    agent_path = write_manifest(
        tmp_path,
        "agent.yaml",
        f"""
        apiVersion: talon.impalasys.com/v1
        kind: Agent
        metadata:
          name: {agent_name}
          namespace: {namespace}
        spec:
            modelPolicy:
              profiles:
                - name: default
                  model:
                    provider: mock
                    name: minimax-m2.7
                    temperature: 0
            systemPrompt: You are a deterministic A2A compatibility test agent.
            a2a:
              agentCard:
                name: A2A Compatibility Agent
                description: AgentCard used by the upstream A2A SDK compatibility test.
                version: 1.0.0
                capabilities:
                  streaming: true
                  pushNotifications: false
                  extendedAgentCard: false
                defaultInputModes:
                  - text/plain
                defaultOutputModes:
                  - text/plain
                skills:
                  - id: answer_compat_question
                    name: Answer compatibility question
                    description: Answers deterministic A2A compatibility prompts.
                    tags:
                      - compatibility
                    examples:
                      - Hello from A2A compatibility CI
                    inputModes:
                      - text/plain
                    outputModes:
                      - text/plain
        """,
    )
    apply_manifest(agent_path, gateway_url, api_key, auth_file)


@pytest.mark.asyncio
async def test_upstream_a2a_sdk_can_discover_send_stream_and_read_task(
    talon_infrastructure, mock_llm_server, tmp_path, test_grpc_port
):
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-a2a-compat-{run_id}"
    agent_name = f"a2a-agent-{run_id}"
    gateway_url = f"http://127.0.0.1:{test_grpc_port}"
    api_key = talon_infrastructure["api_key"]
    access_token = api_key_access_token(test_grpc_port, api_key)
    create_a2a_fixture(
        namespace,
        agent_name,
        tmp_path,
        gateway_url,
        api_key,
        talon_infrastructure["auth_file"],
    )
    await assert_grpc_web_list_namespaces(
        gateway_url,
        access_token,
    )

    agent_card_url = f"http://localhost:{test_grpc_port}/a2a/{namespace}/{agent_name}/agent-card.json"
    async with httpx.AsyncClient(
        timeout=90.0,
        headers={
            "authorization": f"Bearer {access_token}",
            "x-forwarded-proto": "http",
        },
    ) as http_client:
        resolver = card_resolver(http_client, agent_card_url)
        card = await resolver.get_agent_card()

        assert card.name == "A2A Compatibility Agent"
        assert card.protocol_version == "0.3.0"
        assert card.preferred_transport == TransportProtocol.http_json
        assert card.url == f"http://localhost:{test_grpc_port}/a2a/{namespace}/{agent_name}"
        assert card.capabilities.streaming is True
        assert card.default_input_modes == ["text/plain"]
        assert card.default_output_modes == ["text/plain"]
        assert card.skills[0].id == "answer_compat_question"

        config = ClientConfig(
            supported_transports=[TransportProtocol.http_json],
            use_client_preference=True,
            httpx_client=http_client,
        )
        client = ClientFactory(config).create(card)

        context_id = str(uuid.uuid4())
        message = Message(
            role=Role.user,
            parts=[TextPart(text="Hello from A2A compatibility CI")],
            message_id=str(uuid.uuid4()),
            context_id=context_id,
        )

        events = []
        async for event in client.send_message(message):
            event_dump = dump_event(event)
            events.append(event_dump)

        assert events, "upstream A2A SDK did not receive any events from Talon"
        assert get_field(events[0], "kind", "kind") == "task"
        artifact_events = [
            event
            for event in events
            if get_field(event, "kind", "kind") == "artifact-update"
            or event.get("artifactUpdate")
        ]
        assert artifact_events, "Talon did not stream A2A artifact updates"
        streamed_text = "".join(artifact_text(event) for event in artifact_events)
        assert "Hello! I am a mock LLM." in streamed_text
        assert bool_field(artifact_events[0], "append", "append") is False
        artifact_ids = {
            artifact_field(event, "artifact_id", "artifactId")
            for event in artifact_events
        }
        assert artifact_ids == {"response"}
        for event in artifact_events[:-1]:
            assert bool_field(event, "last_chunk", "lastChunk") is False
        if len(artifact_events) > 1:
            assert all(
                bool_field(event, "append", "append")
                for event in artifact_events[1:]
            )
        assert bool_field(artifact_events[-1], "last_chunk", "lastChunk") is True
        assert all("usage" not in str(event).lower() for event in events)
        assert all(status_message_text(event) == "" for event in events)

        final_event = events[-1]
        assert get_field(final_event, "kind", "kind") == "status-update"
        assert final_event.get("final") is True
        task_id = get_field(final_event, "task_id", "taskId")
        assert task_id
        assert get_field(final_event, "task_id", "taskId") == task_id
        assert get_field(final_event, "context_id", "contextId") == context_id
        assert final_event.get("status", {}).get("state") == "completed"

        task = None
        for _ in range(20):
            task_response = await http_client.get(f"{card.url}/v1/tasks/{task_id}")
            task_response.raise_for_status()
            task = task_response.json()
            if task["status"]["state"] == "TASK_STATE_COMPLETED":
                break
            await asyncio.sleep(0.25)
        assert task is not None
        assert task["id"] == task_id
        assert task["contextId"] == context_id
        assert task["status"]["state"] == "TASK_STATE_COMPLETED"
        assert task["status"].get("message") is None
        assert "Hello! I am a mock LLM." in artifact_texts(task)[-1]
        assert "Hello from A2A compatibility CI" in task_history_texts(task)[0]
        assert "Hello! I am a mock LLM." in task_history_texts(task)[-1]
