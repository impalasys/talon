import grpc
import sys
import os
import threading
import time

# Important: Add generated protos to path so "proto.xxx" resolves locally and not to proto_plus
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))

from proto.gateway_pb2_grpc import GatewayServiceStub
from proto.gateway_pb2 import (
    CreateAgentRequest,
    CreateNamespaceRequest,
    CreateSessionRequest,
    GetSessionRequest,
    SendMessageRequest,
    StreamSessionStepsRequest,
)
from proto.manifests_pb2 import AgentDefinition, AgentSpec, Model

STEP_TYPE_TOKEN = 1
STEP_TYPE_REASONING = 6
STEP_TYPE_USAGE = 7


def ensure_namespace(stub, name):
    try:
        stub.CreateNamespace(CreateNamespaceRequest(name=name, recursive=True))
    except grpc.RpcError as err:
        if err.code() != grpc.StatusCode.ALREADY_EXISTS:
            raise


def test_single_turn_chat_sqlite_local_socket(gateway_channel_sqlite, mock_llm_server):
    stub = GatewayServiceStub(gateway_channel_sqlite)

    ensure_namespace(stub, "talon-sqlite-test")

    agent = stub.CreateAgent(
        CreateAgentRequest(
            ns="talon-sqlite-test",
            name="test-llm-agent",
            definition=AgentDefinition(
                custom_spec=AgentSpec(
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
                )
            ),
        )
    )
    assert agent.agent == "test-llm-agent"

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
        if res.state == "IDLE" and len(messages) >= 2:
            success = True
            break

    assert success, "Agent did not reply in time or failed to revert to IDLE"
    agent_message = messages[-1]
    assert agent_message.role == 2
    assert "12" in agent_message.content


def test_streaming_chat_sqlite_local_socket(gateway_channel_sqlite, mock_llm_server):
    stub = GatewayServiceStub(gateway_channel_sqlite)

    ensure_namespace(stub, "talon-sqlite-stream-test")

    stub.CreateAgent(
        CreateAgentRequest(
            ns="talon-sqlite-stream-test",
            name="stream-agent",
            definition=AgentDefinition(
                custom_spec=AgentSpec(
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
                )
            ),
        )
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

    stream_req = StreamSessionStepsRequest(
        agent="stream-agent",
        session_id=session_id,
        ns="talon-sqlite-stream-test",
    )
    events = []
    try:
        saw_reasoning = False
        saw_token = False
        saw_usage = False
        for idx, event in enumerate(stub.StreamSessionSteps(stream_req)):
            events.append(event)
            if event.step_type == STEP_TYPE_REASONING:
                saw_reasoning = True
            if event.step_type == STEP_TYPE_TOKEN:
                saw_token = True
            if event.step_type == STEP_TYPE_USAGE:
                saw_usage = True
            if saw_reasoning and saw_token and saw_usage:
                break
            if idx > 20:
                break
    except grpc.RpcError:
        pass
    sender.join()

    assert len(events) >= 1
    reasoning_events = [event for event in events if event.step_type == STEP_TYPE_REASONING]
    token_events = [event for event in events if event.step_type == STEP_TYPE_TOKEN]
    usage_events = [event for event in events if event.step_type == STEP_TYPE_USAGE]
    assert len(reasoning_events) >= 1
    assert len(token_events) >= 1
    assert len(usage_events) >= 1
    assert "Inspecting the request" in reasoning_events[0].content
    streamed_text = "".join(event.content for event in token_events)
    assert "received" in streamed_text
