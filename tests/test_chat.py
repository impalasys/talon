import pytest
import time
import sys
import os
import grpc
import json
import httpx

# Important: Add generated protos to path so "proto.xxx" resolves locally and not to proto_plus
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))

import conftest
import mock_llm
from proto.gateway_pb2_grpc import GatewayServiceStub
from proto.gateway_pb2 import (
    CreateAgentRequest, 
    CreateSessionRequest, 
    SendMessageRequest,
    GetSessionRequest,
    CreateNamespaceRequest,
    StreamSessionStepsRequest,
    CreateNamespaceKnowledgeRequest,
    GetNamespaceKnowledgeRequest,
    ListNamespaceKnowledgeRequest,
    DeleteNamespaceKnowledgeRequest,
    GetKnowledgeRequest,
    SearchKnowledgeRequest,
    CreateScheduleRequest,
    GetScheduleRequest,
    ListSchedulesRequest,
    DeleteScheduleRequest,
)
from proto.manifests_pb2 import AgentDefinition, AgentSpec, Model, Knowledge, ObjectMeta, KnowledgeSpec
from proto.models_pb2 import Schedule, ScheduleSpec, ScheduleTarget
import threading
import uuid

STEP_TYPE_TOKEN = 1
STEP_TYPE_REASONING = 6
STEP_TYPE_USAGE = 7

@pytest.fixture
def anyio_backend():
    return "asyncio"

def test_single_turn_chat(gateway_channel, mock_llm_server):
    stub = GatewayServiceStub(gateway_channel)
    
    # 0. Create Namespace
    try:
        stub.CreateNamespace(CreateNamespaceRequest(
            name="talon-test",
            recursive=True
        ))
    except grpc.RpcError as e:
        if e.code() != grpc.StatusCode.ALREADY_EXISTS:
            raise

    # 1. Create Agent
    agent_spec = AgentSpec(
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
        system_prompt="You are a helpful test assistant."
    )
    
    agent = stub.CreateAgent(CreateAgentRequest(
        ns="talon-test",
        name="test-llm-agent",
        definition=AgentDefinition(custom_spec=agent_spec),
    ))
    
    assert agent.agent == "test-llm-agent"
    
    # 2. Create Session
    session = stub.CreateSession(CreateSessionRequest(
        agent="test-llm-agent",
        ns="talon-test"
    ))
    session_id = session.session_id
    assert session_id != ""
    
    # Wait for agent creation event to propagate? No, should be instant enough.
    
    # 3. Send Message
    stub.SendMessage(SendMessageRequest(
        agent="test-llm-agent",
        session_id=session_id,
        ns="talon-test",
        message="What is the square root of 144?"
    ))
    
    # 4. Long / Async Poll behavior to wait for IDLE
    max_retries = 30
    delay = 1.0
    
    success = False
    messages = []
    
    print(f"\nPolling for message results in session {session_id}...")
    for i in range(max_retries):
        time.sleep(delay)
        res = stub.GetSession(GetSessionRequest(
            agent="test-llm-agent",
            session_id=session_id,
            ns="talon-test"
        ))
        
        # In our implementation, we change status specifically mapping worker persistence.
        status = res.state
        messages = res.messages
        
        print(f"[{i}/{max_retries}] Session status: {status}, Messages: {len(messages)}")
        
        # We expect a USER message and then an ASSISTANT message
        if status == "IDLE" and len(messages) >= 2:
            success = True
            break
            
    assert success, "Agent did not reply in time or failed to revert to IDLE"
    
    agent_message = messages[-1]
    assert agent_message.role == 2 # MessageRole.ROLE_ASSISTANT
    assert "12" in agent_message.content

def test_streaming_chat(gateway_channel, mock_llm_server):
    stub = GatewayServiceStub(gateway_channel)
    
    try:
        stub.CreateNamespace(CreateNamespaceRequest(name="talon-stream-test", recursive=True))
    except grpc.RpcError as e:
        if e.code() != grpc.StatusCode.ALREADY_EXISTS:
            raise

    agent_spec = AgentSpec(
        model_policy={
            "profiles": [
                {
                    "name": "default",
                    "model": Model(provider="mock", name="minimax", temperature=0.7),
                }
            ]
        },
        system_prompt="Stream me."
    )
    
    agent = stub.CreateAgent(CreateAgentRequest(
        ns="talon-stream-test",
        name="stream-agent",
        definition=AgentDefinition(custom_spec=agent_spec),
    ))
    
    session = stub.CreateSession(CreateSessionRequest(
        agent="stream-agent",
        ns="talon-stream-test"
    ))
    session_id = session.session_id
    
    def send_msg():
        # Delay to ensure the subscriber is fully connected to the emulator
        time.sleep(2.0)
        stub.SendMessage(SendMessageRequest(
            agent="stream-agent",
            session_id=session_id,
            ns="talon-stream-test",
            message="Stream test message"
        ))

    t = threading.Thread(target=send_msg)
    t.start()

    # The stream will block until the worker publishes the events.
    stream_req = StreamSessionStepsRequest(
        agent="stream-agent",
        session_id=session_id,
        ns="talon-stream-test"
    )
    events = []
    try:
        # We limit iteration to prevent infinite block if stream is buggy
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
    except grpc.RpcError as e:
        print("RPC ERROR:", e)
        pass # Stream might close or error
    t.join()
    
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
    usage_payload = json.loads(usage_events[0].payload_json)
    assert usage_payload["reasoning_tokens"] == 6

def test_knowledge_crud_and_search(gateway_channel, mock_llm_server):
    stub = GatewayServiceStub(gateway_channel)
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-knowledge-{run_id}"
    agent_name = f"knowledge-agent-{run_id}"

    stub.CreateNamespace(CreateNamespaceRequest(name=namespace, recursive=True))

    agent_spec = AgentSpec(
        model_policy={
            "profiles": [
                {
                    "name": "default",
                    "model": Model(provider="mock", name="minimax", temperature=0.7),
                }
            ]
        },
        system_prompt="Knowledge test agent."
    )
    stub.CreateAgent(CreateAgentRequest(
        ns=namespace,
        name=agent_name,
        definition=AgentDefinition(custom_spec=agent_spec),
    ))

    created = stub.CreateNamespaceKnowledge(CreateNamespaceKnowledgeRequest(
        ns=namespace,
        knowledge=Knowledge(
            metadata=ObjectMeta(name="guide"),
            spec=KnowledgeSpec(
                path="guide.md",
                content="Talon stores runtime facts in guide documents."
            ),
        ),
    ))
    assert created.knowledge.metadata.name == "guide"

    fetched = stub.GetNamespaceKnowledge(GetNamespaceKnowledgeRequest(
        ns=namespace,
        name="guide",
    ))
    assert fetched.knowledge.spec.path == "guide.md"
    assert "runtime facts" in fetched.knowledge.spec.content

    listed = stub.ListNamespaceKnowledge(ListNamespaceKnowledgeRequest(ns=namespace))
    assert len(listed.knowledge) == 1
    assert listed.knowledge[0].metadata.name == "guide"

    modules = stub.GetKnowledge(GetKnowledgeRequest(
        ns=namespace,
        agent=agent_name,
        path="guide.md",
    ))
    assert len(modules.modules) == 1
    assert modules.modules[0].path == "guide.md"
    assert "guide documents" in modules.modules[0].content

    search = stub.SearchKnowledge(SearchKnowledgeRequest(
        ns=namespace,
        agent=agent_name,
        query="runtime facts",
    ))
    assert len(search.results) >= 1
    assert search.results[0].path == "guide.md"

    deleted = stub.DeleteNamespaceKnowledge(DeleteNamespaceKnowledgeRequest(
        ns=namespace,
        name="guide",
    ))
    assert deleted.success is True

def test_schedule_crud_round_trip(gateway_channel, mock_llm_server):
    stub = GatewayServiceStub(gateway_channel)
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-schedule-{run_id}"
    agent_name = f"schedule-agent-{run_id}"
    schedule_name = f"schedule-{run_id}"

    stub.CreateNamespace(CreateNamespaceRequest(name=namespace, recursive=True))

    agent_spec = AgentSpec(
        model_policy={
            "profiles": [
                {
                    "name": "default",
                    "model": Model(provider="mock", name="minimax", temperature=0.7),
                }
            ]
        },
        system_prompt="Schedule test agent."
    )
    stub.CreateAgent(CreateAgentRequest(
        ns=namespace,
        name=agent_name,
        definition=AgentDefinition(custom_spec=agent_spec),
    ))

    created = stub.CreateSchedule(CreateScheduleRequest(
        ns=namespace,
        schedule=Schedule(
            name=schedule_name,
            spec=ScheduleSpec(
                kind="every",
                interval_seconds=300,
                timezone="UTC",
                target=ScheduleTarget(agent=agent_name, session_mode="new"),
                input_message="Run a periodic check-in",
                enabled=True,
            ),
            labels={"team": "ops"},
        ),
    ))
    assert created.schedule.name == schedule_name
    assert created.schedule.ns == namespace
    assert created.schedule.status.backend_armed is False

    fetched = stub.GetSchedule(GetScheduleRequest(ns=namespace, name=schedule_name))
    assert fetched.schedule.name == schedule_name
    assert fetched.schedule.spec.target.agent == agent_name

    listed = stub.ListSchedules(ListSchedulesRequest(ns=namespace))
    assert len(listed.schedules) == 1
    assert listed.schedules[0].name == schedule_name

    deleted = stub.DeleteSchedule(DeleteScheduleRequest(ns=namespace, name=schedule_name))
    assert deleted.success is True

def test_conftest_binary_helpers_cover_candidate_and_path_resolution(monkeypatch):
    assert list(conftest.binary_candidates("talon_server")) == [
        "talon_server",
        "talon-server",
    ]
    assert list(conftest.binary_candidates("talon-worker")) == [
        "talon-worker",
        "talon_worker",
    ]

    monkeypatch.delenv("BUILD_WORKSPACE_DIRECTORY", raising=False)
    monkeypatch.setattr(conftest, "get_runfile_binary_path", lambda name: None)
    monkeypatch.setattr(conftest.shutil, "which", lambda name: f"/usr/bin/{name}")
    assert conftest.get_binary_path("talon_server") == "/usr/bin/talon_server"


def test_mock_llm_helper_functions_cover_message_and_tool_detection():
    messages = [{"role": "user", "content": "please lookup docs.example.com"}]
    assert mock_llm.last_message(messages) == messages[-1]
    assert mock_llm.last_message([]) == {}
    assert mock_llm.last_message_text(messages) == "please lookup docs.example.com"
    assert mock_llm.last_message_text([{"content": ["not", "a", "string"]}]) == ""
    assert mock_llm.should_emit_tool_call(messages, [{"type": "function"}]) is True
    assert mock_llm.should_emit_tool_call(messages, []) is False
    assert mock_llm.is_tool_followup(
        [{"role": "tool", "tool_call_id": mock_llm.TOOL_CALL_ID}]
    ) is True
    assert mock_llm.is_tool_followup([{"role": "assistant"}]) is False

    response = mock_llm.build_tool_call_response("mock-model")
    tool_call = response["choices"][0]["message"]["tool_calls"][0]
    assert response["model"] == "mock-model"
    assert response["choices"][0]["message"]["content"] == mock_llm.TOOL_PREFACE
    assert tool_call["function"]["name"] == mock_llm.TOOL_NAME
    assert json.loads(tool_call["function"]["arguments"]) == {"query": "docs.example.com"}


@pytest.mark.anyio
async def test_mock_llm_stream_helpers_cover_text_and_tool_chunks():
    tool_chunks = [chunk async for chunk in mock_llm.stream_tool_call_response("mock-model")]
    assert tool_chunks[-1] == "data: [DONE]\n\n"
    assert any(mock_llm.TOOL_NAME in chunk for chunk in tool_chunks)
    assert any(mock_llm.TOOL_PREFACE in chunk for chunk in tool_chunks)

    text_chunks = [chunk async for chunk in mock_llm.stream_text_response("mock-model", "hello world")]
    assert text_chunks[-1] == "data: [DONE]\n\n"
    assert any("hello " in chunk or "world" in chunk for chunk in text_chunks)


@pytest.mark.anyio
async def test_mock_llm_chat_completions_endpoint_covers_json_and_streaming_paths():
    transport = httpx.ASGITransport(app=mock_llm.app)
    async with httpx.AsyncClient(transport=transport, base_url="http://testserver") as client:
        standard = await client.post(
            "/chat/completions",
            json={
                "model": "mock-model",
                "messages": [{"role": "user", "content": "What is the square root of 144?"}],
            },
        )
        assert standard.status_code == 200
        assert "12" in standard.json()["choices"][0]["message"]["content"]

        tool_call = await client.post(
            "/chat/completions",
            json={
                "model": "mock-model",
                "messages": [{"role": "user", "content": "Please lookup docs.example.com"}],
                "tools": [{"type": "function"}],
            },
        )
        assert tool_call.status_code == 200
        assert (
            tool_call.json()["choices"][0]["message"]["tool_calls"][0]["function"]["name"]
            == mock_llm.TOOL_NAME
        )

        followup = await client.post(
            "/chat/completions",
            json={
                "model": "mock-model",
                "messages": [{"role": "tool", "tool_call_id": mock_llm.TOOL_CALL_ID}],
            },
        )
        assert followup.status_code == 200
        assert "docs.example.com" in followup.json()["choices"][0]["message"]["content"]

        async with client.stream(
            "POST",
            "/chat/completions",
            json={
                "model": "mock-model",
                "stream": True,
                "messages": [{"role": "user", "content": "hello"}],
            },
        ) as response:
            body = await response.aread()
        text = body.decode()
        assert response.status_code == 200
        assert "data: [DONE]" in text
        content_chunks = []
        for event in text.split("\n\n"):
            if not event.startswith("data: ") or event == "data: [DONE]":
                continue
            payload = json.loads(event[len("data: "):])
            delta = payload["choices"][0]["delta"]
            if "content" in delta:
                content_chunks.append(delta["content"])
        assert "".join(content_chunks).startswith("Hello! I am a mock LLM.")

if __name__ == '__main__':
    sys.exit(pytest.main(sys.argv[1:] + [__file__]))
