import json
import logging
import threading
import time
import uuid

import grpc

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
    GetSessionRequest,
    SendMessageRequest,
    StreamSessionPartsRequest,
    TalonClient,
)
from talon_client.resources import AgentSpec, McpServerSpec, Model, ResourceSpec


PART_TYPE_TEXT = 1
PART_TYPE_REASONING = 2
PART_TYPE_TOOL_RESULT = 4
PART_TYPE_USAGE = 5
STREAM_TIMEOUT_SECONDS = 30


logger = logging.getLogger(__name__)


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


def test_large_tool_result_is_fetched_from_cas(
    stack: E2EStack,
    client: TalonClient,
) -> None:
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
            message="Please run a blocking lookup docs.example.com and summarize what you found.",
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
    assert "I checked blocking_lookup for docs.example.com." in message_text(assistant)

    tool_results = [
        part for part in assistant.parts if part.part_type == PART_TYPE_TOOL_RESULT
    ]
    assert len(tool_results) == 1
    tool_result = tool_results[0]
    assert tool_result.content == ""
    assert tool_result.object.key.startswith(
        f"cas/{namespace}/sessions/{session_id}/messages/"
    )
    payload = json.loads(tool_result.payload_json)
    assert "output" not in payload
    assert "output_preview" not in payload
    assert payload["output_object_key"] == tool_result.object.key

    fetched = client.cas.GetObject(
        GetCasObjectRequest(
            key=tool_result.object.key,
        )
    )
    hydrated = fetched.data.decode("utf-8")
    assert hydrated.startswith("blocking_lookup result for docs.example.com")
    assert "reference section 079" in hydrated
