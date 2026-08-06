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
    StopSessionGenerationRequest,
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


def mock_control(method: str, path: str, payload: dict | None = None) -> dict:
    response = requests.request(
        method,
        f"http://127.0.0.1:{MOCK_LLM_PORT}{path}",
        json=payload,
        timeout=5,
    )
    response.raise_for_status()
    return response.json()


def wait_for_mock_stream_blocked(*, attempts: int = 100, delay: float = 0.1) -> None:
    for _ in range(attempts):
        if mock_control("GET", "/__control/state").get("blocked"):
            return
        time.sleep(delay)
    raise AssertionError("mock LLM did not block its stream")


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


def test_native_openai_responses_api_handles_reasoning_and_tools(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    namespace = f"talon-responses-{stack.name}-{uuid.uuid4().hex[:8]}"
    ensure_namespace(client, namespace)
    agent_name = "responses-api-agent"
    create_agent_resource(
        client,
        namespace,
        agent_name,
        AgentSpec(
            model_policy={
                "profiles": [
                    {
                        "name": "default",
                        "model": Model(
                            provider="openai",
                            name="minimax/m2.7",
                            temperature=0.0,
                            thinking={
                                "enabled": True,
                                "budget_tokens": 2048,
                                "effort": "high",
                            },
                        ),
                    }
                ]
            },
            system_prompt="Use tools when needed.",
        ),
    )
    mock_control("POST", "/__control/reset")
    session_id = client.sessions.Create(
        CreateSessionRequest(agent=agent_name, ns=namespace)
    ).session_id
    client.sessions.SendMessage(
        SendMessageRequest(
            agent=agent_name,
            session_id=session_id,
            ns=namespace,
            message="lookup docs.example.com",
        )
    )

    for _ in range(30):
        time.sleep(1)
        session = client.sessions.Get(
            GetSessionRequest(agent=agent_name, session_id=session_id, ns=namespace)
        )
        assistant = last_assistant_message(session.messages)
        if session.state == "IDLE" and assistant is not None:
            break
    else:
        raise AssertionError("Responses API agent did not complete")

    state = mock_control("GET", "/__control/state")
    assert state["responses_requests"]
    assert state["responses_requests"][0]["toolNames"]
    assert not state["chat_requests"]
    assert state["responses_requests"][0]["previousResponseId"] is None
    assert state["responses_requests"][0]["input"]
    assert state["responses_requests"][0]["input"][-1]["role"] == "user"
    assert len(state["responses_requests"]) >= 2
    assert state["responses_requests"][1]["previousResponseId"] == "resp_mock_1"
    assert any(
        item.get("type") == "function_call_output"
        for request in state["responses_requests"]
        for item in request["input"]
    )
    assert all(
        "encrypted_content" not in json.dumps(request)
        for request in state["responses_requests"]
    )
    assert assistant is not None
    assert "checked" in message_text(assistant).lower()
    assert session.context_tokens.provider_request_id == "resp_mock_2"
    assert any(
        part.part_type == PART_TYPE_REASONING and part.content
        for part in assistant.parts
    )


def test_stop_generation_cancels_an_inflight_worker_stream(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    """StopGeneration reaches the worker executing a blocked provider stream."""
    namespace = f"talon-stop-{stack.name}-{uuid.uuid4().hex[:8]}"
    agent = "stop-generation-agent"
    ensure_namespace(client, namespace)
    create_agent_resource(
        client,
        namespace,
        agent,
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
            system_prompt="You are a cancellation test assistant.",
        ),
    )
    session_id = client.sessions.Create(
        CreateSessionRequest(agent=agent, ns=namespace)
    ).session_id

    mock_control("POST", "/__control/reset")
    mock_control("POST", "/__control/block_stream_after_chunks", {"chunks": 1})
    try:
        client.sessions.SendMessage(
            SendMessageRequest(
                agent=agent,
                session_id=session_id,
                ns=namespace,
                message="cancel this streaming reply " + " ".join(f"word{i}" for i in range(40)),
            )
        )
        # This is the synchronization barrier: a real worker has consumed a
        # provider chunk and is blocked waiting for the next one.
        wait_for_mock_stream_blocked()

        response = client.sessions.StopGeneration(
            StopSessionGenerationRequest(agent=agent, session_id=session_id, ns=namespace),
            timeout=10,
        )
        assert response.success

        # The executor must observe its cancellation token and finish without
        # releasing the provider's blocked stream. A normal completion cannot
        # satisfy this assertion because the mock remains blocked.
        for _ in range(100):
            session = client.sessions.Get(
                GetSessionRequest(agent=agent, session_id=session_id, ns=namespace)
            )
            if session.state == "IDLE":
                break
            time.sleep(0.1)
        else:
            raise AssertionError("session did not become idle after StopGeneration")
        state = mock_control("GET", "/__control/state")
        assert state["blocked"] is True
        assert state["unblocked"] is False
        assert session.state != "CANCELED"
    finally:
        # Keep the process-global mock server usable even if this test fails.
        mock_control("POST", "/__control/unblock_stream")
        mock_control("POST", "/__control/reset")



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
    assert usage_payload["reasoning_output_tokens"] == 6


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
    assert json.loads(usage_parts[0].payload_json)["reasoning_output_tokens"] == 6


def _run_cas_tool_result_turn(
    stack: E2EStack,
    client: TalonClient,
    *,
    message: str,
    require_summary: bool = True,
    poll_attempts: int = 30,
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
    tool_result_message = None
    for _ in range(poll_attempts):
        response = client.sessions.Get(
            GetSessionRequest(agent=agent_name, session_id=session_id, ns=namespace)
        )
        assistant = last_assistant_message(response.messages)
        tool_result_message = next(
            (
                message
                for message in reversed(response.messages)
                if any(part.part_type == PART_TYPE_TOOL_RESULT for part in message.parts)
            ),
            None,
        )
        if tool_result_message is not None:
            break
        time.sleep(1)

    assert response is not None
    assert assistant is not None
    assert tool_result_message is not None
    if require_summary:
        assert "I checked blocking_lookup for docs.example.com." in message_text(assistant)

    tool_results = [
        part
        for part in tool_result_message.parts
        if part.part_type == PART_TYPE_TOOL_RESULT
    ]
    assert len(tool_results) == 1
    assert tool_results[0].content == ""
    assert tool_results[0].object.key.startswith(
        f"cas/{namespace}/sessions/{session_id}/messages/"
    )
    payload = json.loads(tool_results[0].payload_json)
    assert "output" not in payload
    assert "output_preview" not in payload
    assert (
        payload["tool_output"]["content_parts"][0]["object_ref"]["key"]
        == tool_results[0].object.key
    )
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
            poll_attempts=90,
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
