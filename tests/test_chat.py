import gzip
import io
import json
import logging
import threading
import time
import uuid

import boto3
import grpc
import requests
import zstandard
from google.protobuf.struct_pb2 import ListValue, Value

from e2e.blackbox import (
    create_agent_resource,
    create_resource,
    ensure_namespace,
    last_assistant_message,
    message_text,
)
from e2e.stack import E2EStack, MOCK_LLM_PORT
from talon_client import (
    CreateSessionRequest,
    GetCasObjectRequest,
    GetResourceRequest,
    GetSessionRequest,
    ListResourcesRequest,
    ListSessionsRequest,
    SendMessageRequest,
    StreamSessionPartsRequest,
    TalonClient,
)
from talon_client.resources import AgentSpec, McpServerSpec, Model, ResourceSpec
from talon_client.resources import A2A, Connection, ConnectionRef, InternalConnectionRef


PART_TYPE_TEXT = 1
PART_TYPE_REASONING = 2
PART_TYPE_TOOL_RESULT = 4
PART_TYPE_USAGE = 5
STREAM_TIMEOUT_SECONDS = 30


logger = logging.getLogger(__name__)


def _capability_values(*values: str) -> ListValue:
    return ListValue(values=[Value(string_value=value) for value in values])


def _internal_a2a(connection: str, namespace: str, agent: str) -> A2A:
    return A2A(
        connections=[
            Connection(
                name=connection,
                target=ConnectionRef(
                    internal=InternalConnectionRef(namespace=namespace, agent=agent)
                ),
            )
        ]
    )


def _cas_response_bytes(response) -> bytes:
    if response.signed_url:
        downloaded = requests.get(response.signed_url, timeout=30)
        downloaded.raise_for_status()
        return downloaded.content
    return response.data


def _cas_tool_result_text(response) -> str:
    data = _cas_response_bytes(response)
    encoding = (
        response.content_encoding
        or response.metadata.get("content_encoding", "")
    ).lower()
    if encoding == "zstd":
        with zstandard.ZstdDecompressor().stream_reader(io.BytesIO(data)) as reader:
            data = reader.read()
    elif encoding == "gzip":
        data = gzip.decompress(data)
    return data.decode("utf-8")


def test_single_turn_chat(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    # Verify a basic request/response chat turn through the worker and confirm
    # the session returns to IDLE with the expected assistant answer.
    namespace = f"talon-chat-{stack.name}-{uuid.uuid4().hex[:8]}"
    ensure_namespace(client, namespace)
    agent = create_agent_resource(
        client,
        namespace,
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

    session_id = client.sessions.Create(
        CreateSessionRequest(agent="test-llm-agent", ns=namespace)
    ).session_id
    assert session_id != ""

    client.sessions.SendMessage(
        SendMessageRequest(
            agent="test-llm-agent",
            session_id=session_id,
            ns=namespace,
            message="What is the square root of 144?",
        )
    )

    success = False
    messages = []
    for _ in range(30):
        time.sleep(1)
        res = client.sessions.Get(
            GetSessionRequest(
                agent="test-llm-agent",
                session_id=session_id,
                ns=namespace,
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


def test_delegate_task_creates_durable_child_session(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    namespace_suffix = f"{stack.name}-{uuid.uuid4().hex[:8]}"
    owner_namespace = f"talon-delegate-owner-{namespace_suffix}"
    worker_namespace = f"talon-delegate-worker-{namespace_suffix}"
    ensure_namespace(client, owner_namespace)
    ensure_namespace(client, worker_namespace)
    create_agent_resource(
        client,
        worker_namespace,
        "worker-agent",
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
            system_prompt="You complete assigned tasks and reply concisely.",
        ),
    )
    create_agent_resource(
        client,
        owner_namespace,
        "owner-agent",
        AgentSpec(
            capabilities={
                "tasks": _capability_values("create", "inspect"),
                "sessions": _capability_values("read:messages"),
            },
            a2a=_internal_a2a("worker", worker_namespace, "worker-agent"),
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
            system_prompt="Delegate suitable work with the delegate_task tool.",
        ),
    )

    owner_session_id = client.sessions.Create(
        CreateSessionRequest(agent="owner-agent", ns=owner_namespace)
    ).session_id
    client.sessions.SendMessage(
        SendMessageRequest(
            agent="owner-agent",
            session_id=owner_session_id,
            ns=owner_namespace,
            message=(
                "Please delegate onboarding task to the worker connection."
            ),
        )
    )

    owner_done = False
    task_name = ""
    owner_messages = []
    for _ in range(45):
        time.sleep(1)
        owner = client.sessions.Get(
            GetSessionRequest(
                agent="owner-agent",
                session_id=owner_session_id,
                ns=owner_namespace,
            )
        )
        owner_messages = owner.messages
        owner_text = "\n".join(message_text(message) for message in owner_messages)
        if (
            owner.state == "IDLE"
            and "delegated the onboarding task" in owner_text.lower()
        ):
            owner_done = True
            break

    assert owner_done, "owner did not call delegate_task and finish"
    task_resources = list(
        client.resources.List(
            ListResourcesRequest(ns=owner_namespace, kind="Task")
        ).resources
    )
    assert len(task_resources) == 1
    task_name = task_resources[0].metadata.name

    task = client.resources.Get(
        GetResourceRequest(ns=owner_namespace, kind="Task", name=task_name)
    ).resource
    assert task.status.task.phase in (2, 4)  # RUNNING or already NEEDS_REVIEW.
    child_session_id = task.status.task.execution_ref.session_id
    assert child_session_id
    assert task.status.task.execution_ref.name == "worker-agent"
    assert task.spec.task.owner.namespace == owner_namespace
    assert task.spec.task.delegate.namespace == worker_namespace
    assert (
        task_resources[0].metadata.labels.get("talon.impalasys.com/a2a-connection")
        == "worker"
    )

    worker_sessions = client.sessions.List(
        ListSessionsRequest(ns=worker_namespace, agent="worker-agent")
    )
    assert child_session_id in worker_sessions.session_ids

    worker_done = False
    worker_messages = []
    for _ in range(45):
        time.sleep(1)
        worker = client.sessions.Get(
            GetSessionRequest(
                agent="worker-agent",
                session_id=child_session_id,
                ns=worker_namespace,
            )
        )
        worker_messages = worker.messages
        if worker.state == "IDLE" and last_assistant_message(worker_messages) is not None:
            worker_done = True
            break

    assert worker_done, "delegated worker session did not finish"
    first_worker_message = worker_messages[0]
    assert (
        first_worker_message.labels["talon.impalasys.com/task-name"] == task_name
    )
    assert (
        first_worker_message.labels["talon.impalasys.com/owner-namespace"]
        == owner_namespace
    )
    assert "Create a reviewed onboarding checklist." in message_text(first_worker_message)

    reviewed_task = None
    for _ in range(15):
        reviewed_task = client.resources.Get(
            GetResourceRequest(ns=owner_namespace, kind="Task", name=task_name)
        ).resource
        if reviewed_task.status.task.phase == 4:  # TASK_PHASE_NEEDS_REVIEW
            break
        time.sleep(1)

    assert reviewed_task is not None
    assert reviewed_task.status.task.phase == 4
    assert "onboarding" in reviewed_task.status.task.progress_summary.lower()
    assert "artifact" in reviewed_task.status.task.progress_summary.lower()
    assert reviewed_task.status.task.result_artifacts
    artifact_uri = (
        reviewed_task.status.task.result_artifacts[0].metadata.get("artifact_uri", "")
    )
    assert artifact_uri.startswith(f"artifact://{worker_namespace}/worker-agent/")

    worker_tool_results = [
        part
        for message in worker_messages
        for part in message.parts
        if part.part_type == PART_TYPE_TOOL_RESULT
    ]
    assert worker_tool_results, "worker should create an artifact"
    worker_artifact_uri = ""
    for part in worker_tool_results:
        payload = json.loads(part.payload_json or "{}")
        output = payload.get("output", "")
        if output:
            output_json = json.loads(output)
            worker_artifact_uri = output_json.get("artifactUri", "")
            if worker_artifact_uri:
                break
    assert worker_artifact_uri == artifact_uri

    owner_woke = False
    for _ in range(45):
        time.sleep(1)
        owner = client.sessions.Get(
            GetSessionRequest(
                agent="owner-agent",
                session_id=owner_session_id,
                ns=owner_namespace,
            )
        )
        owner_text = "\n".join(message_text(message) for message in owner.messages)
        if (
            owner.state == "IDLE"
            and "Delegated Task is ready for review." in owner_text
            and artifact_uri in owner_text
        ):
            owner_woke = True
            break

    assert owner_woke, "owner was not woken when delegated Task became ready"

    client.sessions.SendMessage(
        SendMessageRequest(
            agent="owner-agent",
            session_id=owner_session_id,
            ns=owner_namespace,
            message=f"Please read this delegated artifact {artifact_uri}",
        )
    )
    owner_read_messages = []
    for _ in range(45):
        time.sleep(1)
        owner = client.sessions.Get(
            GetSessionRequest(
                agent="owner-agent",
                session_id=owner_session_id,
                ns=owner_namespace,
            )
        )
        owner_read_messages = owner.messages
        owner_tool_results = [
            part
            for message in owner_read_messages
            for part in message.parts
            if part.part_type == PART_TYPE_TOOL_RESULT
        ]
        read_outputs = []
        for part in owner_tool_results:
            payload = json.loads(part.payload_json or "{}")
            output = payload.get("output", "")
            if output:
                read_outputs.append(json.loads(output))
        if owner.state == "IDLE" and any(
            "Onboarding checklist" in output.get("content", "")
            for output in read_outputs
        ):
            break

    assert owner_read_messages, "owner did not process artifact read"
    owner_tool_results = [
        part
        for message in owner_read_messages
        for part in message.parts
        if part.part_type == PART_TYPE_TOOL_RESULT
    ]
    read_outputs = [
        json.loads(json.loads(part.payload_json or "{}").get("output", "{}"))
        for part in owner_tool_results
        if json.loads(part.payload_json or "{}").get("output")
    ]
    assert any(
        "Onboarding checklist" in output.get("content", "")
        for output in read_outputs
    ), "owner could not read child artifact"


def test_legal_document_refinement_delegation_returns_redline_artifact(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    namespace_suffix = f"{stack.name}-{uuid.uuid4().hex[:8]}"
    coordinator_namespace = f"talon-legal-coordinator-{namespace_suffix}"
    reviewer_namespace = f"talon-legal-reviewer-{namespace_suffix}"
    ensure_namespace(client, coordinator_namespace)
    ensure_namespace(client, reviewer_namespace)
    create_agent_resource(
        client,
        reviewer_namespace,
        "legal-reviewer-agent",
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
            system_prompt=(
                "You are a legal reviewer. Produce redline artifacts when "
                "assigned a Talon Task."
            ),
        ),
    )
    create_agent_resource(
        client,
        coordinator_namespace,
        "legal-coordinator-agent",
        AgentSpec(
            capabilities={
                "tasks": _capability_values("create", "inspect"),
                "sessions": _capability_values("read:messages"),
            },
            a2a=_internal_a2a(
                "legal-reviewer",
                reviewer_namespace,
                "legal-reviewer-agent",
            ),
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
            system_prompt=(
                "You are a legal coordinator. Delegate legal refinement work "
                "with delegate_task, then review delegated artifacts."
            ),
        ),
    )

    coordinator_session_id = client.sessions.Create(
        CreateSessionRequest(agent="legal-coordinator-agent", ns=coordinator_namespace)
    ).session_id
    client.sessions.SendMessage(
        SendMessageRequest(
            agent="legal-coordinator-agent",
            session_id=coordinator_session_id,
            ns=coordinator_namespace,
            message=(
                "Please delegate legal document refinement task to the "
                "legal-reviewer connection."
            ),
        )
    )

    task_name = ""
    for _ in range(45):
        time.sleep(1)
        coordinator = client.sessions.Get(
            GetSessionRequest(
                agent="legal-coordinator-agent",
                session_id=coordinator_session_id,
                ns=coordinator_namespace,
            )
        )
        coordinator_text = "\n".join(
            message_text(message) for message in coordinator.messages
        )
        if (
            coordinator.state == "IDLE"
            and "delegated the legal document refinement task"
            in coordinator_text.lower()
        ):
            break
    else:
        raise AssertionError("legal coordinator did not delegate refinement work")

    task_resources = list(
        client.resources.List(
            ListResourcesRequest(ns=coordinator_namespace, kind="Task")
        ).resources
    )
    assert len(task_resources) == 1
    task_name = task_resources[0].metadata.name
    task = client.resources.Get(
        GetResourceRequest(ns=coordinator_namespace, kind="Task", name=task_name)
    ).resource
    assert task.spec.task.owner.namespace == coordinator_namespace
    assert task.spec.task.delegate.namespace == reviewer_namespace
    assert task.spec.task.delegate.name == "legal-reviewer-agent"
    child_session_id = task.status.task.execution_ref.session_id
    assert child_session_id

    for _ in range(45):
        time.sleep(1)
        reviewer = client.sessions.Get(
            GetSessionRequest(
                agent="legal-reviewer-agent",
                session_id=child_session_id,
                ns=reviewer_namespace,
            )
        )
        if reviewer.state == "IDLE" and last_assistant_message(reviewer.messages):
            break
    else:
        raise AssertionError("legal reviewer child session did not finish")

    reviewed_task = None
    for _ in range(20):
        reviewed_task = client.resources.Get(
            GetResourceRequest(ns=coordinator_namespace, kind="Task", name=task_name)
        ).resource
        if reviewed_task.status.task.phase == 4:  # TASK_PHASE_NEEDS_REVIEW
            break
        time.sleep(1)

    assert reviewed_task is not None
    assert reviewed_task.status.task.phase == 4
    assert reviewed_task.status.task.result_artifacts
    artifact_uri = reviewed_task.status.task.result_artifacts[0].metadata.get(
        "artifact_uri",
        "",
    )
    assert artifact_uri.startswith(
        f"artifact://{reviewer_namespace}/legal-reviewer-agent/"
    )
    assert (
        reviewed_task.status.task.result_artifacts[0].metadata.get("content_type")
        == "legal_redline"
    )

    for _ in range(45):
        time.sleep(1)
        coordinator = client.sessions.Get(
            GetSessionRequest(
                agent="legal-coordinator-agent",
                session_id=coordinator_session_id,
                ns=coordinator_namespace,
            )
        )
        coordinator_text = "\n".join(
            message_text(message) for message in coordinator.messages
        )
        if (
            coordinator.state == "IDLE"
            and "Delegated Task is ready for review." in coordinator_text
            and artifact_uri in coordinator_text
        ):
            break
    else:
        raise AssertionError("coordinator was not woken with the redline artifact")

    client.sessions.SendMessage(
        SendMessageRequest(
            agent="legal-coordinator-agent",
            session_id=coordinator_session_id,
            ns=coordinator_namespace,
            message=f"Please read this delegated artifact {artifact_uri}",
        )
    )

    read_outputs = []
    for _ in range(45):
        time.sleep(1)
        coordinator = client.sessions.Get(
            GetSessionRequest(
                agent="legal-coordinator-agent",
                session_id=coordinator_session_id,
                ns=coordinator_namespace,
            )
        )
        read_outputs = []
        for message in coordinator.messages:
            for part in message.parts:
                if part.part_type != PART_TYPE_TOOL_RESULT:
                    continue
                payload = json.loads(part.payload_json or "{}")
                output = payload.get("output", "")
                if output:
                    read_outputs.append(json.loads(output))
        if coordinator.state == "IDLE" and any(
            "Mutual NDA fallback clause redline" in output.get("content", "")
            for output in read_outputs
        ):
            break

    assert any(
        "same degree of care" in output.get("content", "")
        and "reasonable care" in output.get("content", "")
        for output in read_outputs
    ), "coordinator could not read the delegated legal redline artifact"


def test_streaming_chat(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    # Verify streamed session parts include reasoning, text tokens, and usage
    # metadata for a normal assistant response.
    namespace = f"talon-stream-{stack.name}-{uuid.uuid4().hex[:8]}"
    ensure_namespace(client, namespace)
    create_agent_resource(
        client,
        namespace,
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

    session_id = client.sessions.Create(
        CreateSessionRequest(agent="stream-agent", ns=namespace)
    ).session_id

    def send_msg() -> None:
        time.sleep(2.0)
        client.sessions.SendMessage(
            SendMessageRequest(
                agent="stream-agent",
                session_id=session_id,
                ns=namespace,
                message="Stream test message",
            )
        )

    sender = threading.Thread(target=send_msg)
    sender.start()

    stream_req = StreamSessionPartsRequest(
        agent="stream-agent",
        session_id=session_id,
        ns=namespace,
    )
    events = []
    try:
        saw_reasoning = False
        saw_token = False
        saw_usage = False
        for idx, event in enumerate(
            client.sessions.StreamParts(stream_req, timeout=STREAM_TIMEOUT_SECONDS)
        ):
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
    except grpc.RpcError as err:
        logger.debug("stream ended: %s", err)
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
    usage_payload = json.loads(usage_events[0].part.payload_json)
    assert usage_payload["reasoning_tokens"] == 6


def test_streaming_chat_persists_coarse_session_message_parts(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    # Regression coverage for streamed durable assembly: live reasoning/text
    # deltas may be emitted in several batches, but the committed assistant
    # message should retain coarse semantic parts.
    namespace = f"talon-stream-parts-{stack.name}-{uuid.uuid4().hex[:8]}"
    ensure_namespace(client, namespace)
    create_agent_resource(
        client,
        namespace,
        "stream-parts-agent",
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
            system_prompt="Stream durable parts.",
        ),
    )

    session_id = client.sessions.Create(
        CreateSessionRequest(agent="stream-parts-agent", ns=namespace)
    ).session_id

    def send_msg() -> None:
        time.sleep(2.0)
        client.sessions.SendMessage(
            SendMessageRequest(
                agent="stream-parts-agent",
                session_id=session_id,
                ns=namespace,
                message="hello",
            )
        )

    sender = threading.Thread(target=send_msg)
    sender.start()

    stream_req = StreamSessionPartsRequest(
        agent="stream-parts-agent",
        session_id=session_id,
        ns=namespace,
    )
    live_reasoning_events = []
    live_text_events = []
    try:
        for idx, event in enumerate(
            client.sessions.StreamParts(stream_req, timeout=STREAM_TIMEOUT_SECONDS)
        ):
            if event.part.part_type == PART_TYPE_REASONING:
                live_reasoning_events.append(event)
            if event.part.part_type == PART_TYPE_TEXT:
                live_text_events.append(event)
            if event.part.part_type == PART_TYPE_USAGE:
                break
            if idx > 30:
                break
    except grpc.RpcError as err:
        logger.debug("stream ended: %s", err)
    sender.join()

    assert len(live_reasoning_events) >= 1
    assert len(live_text_events) >= 1

    response = None
    for _ in range(30):
        response = client.sessions.Get(
            GetSessionRequest(
                agent="stream-parts-agent",
                session_id=session_id,
                ns=namespace,
            )
        )
        if response.state == "IDLE" and last_assistant_message(response.messages):
            break
        time.sleep(1)

    assert response is not None
    assert response.state == "IDLE"
    assistant = last_assistant_message(response.messages)
    assert assistant is not None

    reasoning_parts = [
        part for part in assistant.parts if part.part_type == PART_TYPE_REASONING
    ]
    text_parts = [part for part in assistant.parts if part.part_type == PART_TYPE_TEXT]
    usage_parts = [part for part in assistant.parts if part.part_type == PART_TYPE_USAGE]

    assert [part.part_type for part in assistant.parts] == [
        PART_TYPE_REASONING,
        PART_TYPE_TEXT,
        PART_TYPE_USAGE,
    ]
    assert len(reasoning_parts) == 1
    assert reasoning_parts[0].content == (
        "Inspecting the request. Planning a concise answer. "
    )
    assert len(text_parts) == 1
    assert text_parts[0].content == (
        "Hello! I am a mock LLM. How can I assist you today?"
    )
    assert len(usage_parts) == 1
    assert json.loads(usage_parts[0].payload_json)["reasoning_tokens"] == 6


def _run_cas_tool_result_turn(
    stack: E2EStack,
    client: TalonClient,
    *,
    message: str,
    require_summary: bool = True,
):
    namespace = f"talon-cas-tool-{stack.name}-{uuid.uuid4().hex[:8]}"
    agent_name = "cas-tool-agent"
    mcp_server = "durable-slow"
    ensure_namespace(client, namespace)
    create_resource(
        client,
        namespace,
        "McpServer",
        mcp_server,
        ResourceSpec(
            mcp_server=McpServerSpec(
                transport="http",
                target=f"http://127.0.0.1:{MOCK_LLM_PORT}/mcp",
            )
        ),
    )
    create_agent_resource(
        client,
        namespace,
        agent_name,
        AgentSpec(
            mcp_server_refs=[mcp_server],
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
            system_prompt="Use the MCP lookup tool when asked.",
        ),
    )

    session_id = client.sessions.Create(
        CreateSessionRequest(agent=agent_name, ns=namespace)
    ).session_id
    client.sessions.SendMessage(
        SendMessageRequest(
            agent=agent_name,
            session_id=session_id,
            ns=namespace,
            message=message,
        )
    )

    response = None
    assistant = None
    for _ in range(30):
        response = client.sessions.Get(
            GetSessionRequest(agent=agent_name, session_id=session_id, ns=namespace)
        )
        assistant = last_assistant_message(response.messages)
        if assistant is not None and any(
            part.part_type == PART_TYPE_TOOL_RESULT for part in assistant.parts
        ):
            break
        time.sleep(1)

    assert response is not None
    assert assistant is not None
    if require_summary:
        assert "I checked blocking_lookup for docs.example.com." in message_text(assistant)

    tool_results = [
        part for part in assistant.parts if part.part_type == PART_TYPE_TOOL_RESULT
    ]
    assert len(tool_results) == 1
    assert tool_results[0].content == ""
    assert tool_results[0].object.key.startswith(
        f"cas/{namespace}/sessions/{session_id}/messages/"
    )
    payload = json.loads(tool_results[0].payload_json)
    assert "output" not in payload
    assert "output_preview" not in payload
    assert payload["output_object_key"] == tool_results[0].object.key
    return namespace, session_id, tool_results[0]


def test_large_tool_result_is_fetched_from_cas(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    _namespace, _session_id, tool_result = _run_cas_tool_result_turn(
        stack,
        client,
        message="Please run a blocking lookup docs.example.com and summarize what you found.",
    )

    fetched = client.cas.GetObject(
        GetCasObjectRequest(
            key=tool_result.object.key,
        )
    )
    hydrated = _cas_tool_result_text(fetched)
    assert hydrated.startswith("blocking_lookup result for docs.example.com")
    assert "reference section 079" in hydrated


def test_super_large_tool_result_uses_s3_object_store_on_aws_stack(
    aws_local_stack: E2EStack,
) -> None:
    raw_channel, channel = aws_local_stack.channel()
    try:
        client = TalonClient(channel)
        _namespace, _session_id, tool_result = _run_cas_tool_result_turn(
            aws_local_stack,
            client,
            message=(
                "Please run a blocking lookup docs.example.com for a super large "
                "super-large-docs.example.com result and summarize what you found."
            ),
            require_summary=False,
        )

        fetched = client.cas.GetObject(GetCasObjectRequest(key=tool_result.object.key))
        hydrated = _cas_tool_result_text(fetched)
        assert hydrated.startswith(
            "blocking_lookup result for super-large-docs.example.com"
        )
        assert "reference section 00000" in hydrated
        assert "CONTENT TRUNCATED DUE TO LENGTH LIMIT" in hydrated
        assert len(hydrated.encode("utf-8")) >= 1_000_000

        s3 = boto3.client(
            "s3",
            endpoint_url=aws_local_stack.metadata["localstack_endpoint"],
            region_name="us-east-1",
            aws_access_key_id="test",
            aws_secret_access_key="test",
        )
        stored = s3.get_object(
            Bucket=aws_local_stack.metadata["s3_bucket"],
            Key=f"{aws_local_stack.metadata['s3_prefix']}/{tool_result.object.key}",
        )
        assert stored["Body"].read()
        assert stored["Metadata"]["kind"] == "tool_result"
        assert f"/sessions/{_session_id}/" in tool_result.object.key
        assert "session_id" not in stored["Metadata"]
        assert stored["Metadata"]["uncompressed_size_bytes"] == str(
            len(hydrated.encode("utf-8"))
        )
    finally:
        raw_channel.close()
