import json
import logging
import threading
import time
import uuid

import grpc

from e2e.blackbox import (
    create_agent_resource,
    ensure_namespace,
    last_assistant_message,
    message_text,
)
from e2e.stack import E2EStack
from talon_client import (
    CreateSessionRequest,
    GetSessionRequest,
    SendMessageRequest,
    StreamSessionPartsRequest,
    TalonClient,
)
from talon_client.resources import AgentSpec, Model


PART_TYPE_TEXT = 1
PART_TYPE_REASONING = 2
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
