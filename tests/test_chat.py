import pytest
import time
import sys
import os
import grpc

# Important: Add generated protos to path so "proto.xxx" resolves locally and not to proto_plus
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))

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
from proto.events_pb2 import STEP_TYPE_TOKEN
import threading
import uuid

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
        for idx, event in enumerate(stub.StreamSessionSteps(stream_req)):
            events.append(event)
            if event.step_type == STEP_TYPE_TOKEN:
                # The mock LLM only emits one token chunk, so we break after receiving it
                break
            if idx > 10:
                break
    except grpc.RpcError as e:
        print("RPC ERROR:", e)
        pass # Stream might close or error
    t.join()
    
    assert len(events) >= 1
    token_events = [event for event in events if event.step_type == STEP_TYPE_TOKEN]
    assert len(token_events) >= 1
    assert "received" in token_events[0].content

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

if __name__ == '__main__':
    sys.exit(pytest.main(sys.argv[1:] + [__file__]))
