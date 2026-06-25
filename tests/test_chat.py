import pytest
import time
import grpc
import json
import httpx
import sys

import conftest
import mock_llm
from talon_client import (
    TalonClient,
    CreateResourceRequest,
    CreateNamespaceRequest,
    DeleteResourceRequest,
    GetResourceRequest,
    GetKnowledgeRequest,
    SearchKnowledgeRequest,
    ListResourcesRequest,
    CreateSessionRequest,
    GetSessionRequest,
    SendMessageRequest,
    StreamSessionPartsRequest,
    CreateWorkflowRunRequest,
    GetWorkflowRunRequest,
    ListWorkflowRunsRequest,
    StreamWorkflowEventsRequest,
)
from talon_client.resources import (
    AgentSpec,
    KnowledgeSpec,
    Model,
    ResourceManifest,
    ResourceMeta,
    ResourceSpec,
    ScheduleSpec,
    ScheduleTarget,
    WorkflowSpec,
    WorkflowStep,
)
import threading
import uuid

PART_TYPE_TEXT = 1
PART_TYPE_REASONING = 2
PART_TYPE_USAGE = 5

def message_text(message):
    return "".join(part.content for part in message.parts if part.part_type == PART_TYPE_TEXT)

def create_resource(stub, ns, kind, name, spec):
    return stub.resources.Create(CreateResourceRequest(
        ns=ns,
        manifest=ResourceManifest(
            api_version="talon.impalasys.com/v1",
            kind=kind,
            metadata=ResourceMeta(name=name, namespace=ns),
            spec=spec,
        ),
    )).resource

def create_agent_resource(stub, ns, name, spec):
    return create_resource(
        stub,
        ns,
        "Agent",
        name,
        ResourceSpec(agent=spec),
    )


def wait_for_worker_endpoint(stub, expected_url, attempts=30, delay=1):
    last_urls = []
    for _ in range(attempts):
        resources = list(stub.resources.List(ListResourcesRequest(ns="Sys", kind="Worker")).resources)
        last_urls = [
            endpoint.url
            for resource in resources
            if resource.status and resource.status.worker.phase == "ready"
            for endpoint in resource.status.worker.endpoints
        ]
        if expected_url in last_urls:
            return
        time.sleep(delay)
    raise AssertionError(
        f"Timed out waiting for worker endpoint {expected_url!r}; saw {last_urls!r}"
    )


@pytest.fixture
def anyio_backend():
    return "asyncio"


def test_worker_registers_local_endpoint(gateway_channel, talon_infrastructure):
    stub = TalonClient(gateway_channel)
    wait_for_worker_endpoint(stub, "http://127.0.0.1:8081")


def test_single_turn_chat(gateway_channel, mock_llm_server):
    stub = TalonClient(gateway_channel)
    
    # 0. Create Namespace
    try:
        stub.namespaces.Create(CreateNamespaceRequest(
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
    
    agent = create_agent_resource(stub, "talon-test", "test-llm-agent", agent_spec)
    
    assert agent.metadata.name == "test-llm-agent"
    
    # 2. Create Session
    session = stub.sessions.Create(CreateSessionRequest(
        agent="test-llm-agent",
        ns="talon-test"
    ))
    session_id = session.session_id
    assert session_id != ""
    
    # Wait for agent creation event to propagate? No, should be instant enough.
    
    # 3. Send Message
    stub.sessions.SendMessage(SendMessageRequest(
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
        res = stub.sessions.Get(GetSessionRequest(
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
    assert "12" in message_text(agent_message)

def test_streaming_chat(gateway_channel, mock_llm_server):
    stub = TalonClient(gateway_channel)
    
    try:
        stub.namespaces.Create(CreateNamespaceRequest(name="talon-stream-test", recursive=True))
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
    
    create_agent_resource(stub, "talon-stream-test", "stream-agent", agent_spec)
    
    session = stub.sessions.Create(CreateSessionRequest(
        agent="stream-agent",
        ns="talon-stream-test"
    ))
    session_id = session.session_id
    
    def send_msg():
        # Delay to ensure the subscriber is fully connected to the emulator
        time.sleep(2.0)
        stub.sessions.SendMessage(SendMessageRequest(
            agent="stream-agent",
            session_id=session_id,
            ns="talon-stream-test",
            message="Stream test message"
        ))

    t = threading.Thread(target=send_msg)
    t.start()

    # The stream will block until the worker publishes the events.
    stream_req = StreamSessionPartsRequest(
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
        for idx, event in enumerate(stub.sessions.StreamParts(stream_req)):
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
    except grpc.RpcError as e:
        print("RPC ERROR:", e)
        pass # Stream might close or error
    t.join()
    
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

def test_knowledge_crud_and_search(gateway_channel, mock_llm_server):
    stub = TalonClient(gateway_channel)
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-knowledge-{run_id}"
    agent_name = f"knowledge-agent-{run_id}"

    stub.namespaces.Create(CreateNamespaceRequest(name=namespace, recursive=True))

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
    create_agent_resource(stub, namespace, agent_name, agent_spec)

    created = create_resource(
        stub,
        namespace,
        "Knowledge",
        "guide",
        ResourceSpec(
            knowledge=KnowledgeSpec(
                path="guide.md",
                content="Talon stores runtime facts in guide documents.",
            )
        ),
    )
    assert created.metadata.name == "guide"

    fetched = stub.resources.Get(GetResourceRequest(
        ns=namespace,
        kind="Knowledge",
        name="guide",
    ))
    assert fetched.resource.spec.knowledge.path == "guide.md"
    assert "runtime facts" in fetched.resource.spec.knowledge.content

    listed = stub.resources.List(ListResourcesRequest(ns=namespace, kind="Knowledge"))
    assert len(listed.resources) == 1
    assert listed.resources[0].metadata.name == "guide"

    modules = stub.knowledge.Get(GetKnowledgeRequest(
        ns=namespace,
        agent=agent_name,
        path="guide.md",
    ))
    assert len(modules.modules) == 1
    assert modules.modules[0].path == "guide.md"
    assert "guide documents" in modules.modules[0].content

    search = None
    for _ in range(30):
        search = stub.knowledge.Search(SearchKnowledgeRequest(
            ns=namespace,
            agent=agent_name,
            query="runtime facts",
            limit=10,
        ), timeout=10)
        if search.search_results:
            break
        time.sleep(1)

    assert search is not None
    assert len(search.results) >= 1
    assert search.results[0].path == "guide.md"
    assert len(search.search_results) >= 1
    assert search.search_results[0].document.source.kind == "Knowledge"

    deleted = stub.resources.Delete(DeleteResourceRequest(
        ns=namespace,
        kind="Knowledge",
        name="guide",
    ))
    assert deleted.success is True

def test_schedule_crud_round_trip(gateway_channel, mock_llm_server):
    stub = TalonClient(gateway_channel)
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-schedule-{run_id}"
    agent_name = f"schedule-agent-{run_id}"
    schedule_name = f"schedule-{run_id}"

    stub.namespaces.Create(CreateNamespaceRequest(name=namespace, recursive=True))

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
    create_agent_resource(stub, namespace, agent_name, agent_spec)

    created = create_resource(
        stub,
        namespace,
        "Schedule",
        schedule_name,
        ResourceSpec(
            schedule=ScheduleSpec(
                kind="every",
                interval_seconds=300,
                timezone="UTC",
                target=ScheduleTarget(agent=agent_name, session_mode="new"),
                input_message="Run a periodic check-in",
                enabled=True,
            ),
        ),
    )
    assert created.metadata.name == schedule_name
    assert created.metadata.namespace == namespace
    assert created.status.schedule.backend_armed is False

    fetched = stub.resources.Get(GetResourceRequest(ns=namespace, kind="Schedule", name=schedule_name))
    assert fetched.resource.metadata.name == schedule_name
    assert fetched.resource.spec.schedule.target.agent == agent_name

    listed = stub.resources.List(ListResourcesRequest(ns=namespace, kind="Schedule"))
    assert len(listed.resources) == 1
    assert listed.resources[0].metadata.name == schedule_name

    deleted = stub.resources.Delete(DeleteResourceRequest(ns=namespace, kind="Schedule", name=schedule_name))
    assert deleted.success is True

def test_workflow_transform_run_completes_through_worker(gateway_channel, mock_llm_server):
    stub = TalonClient(gateway_channel)
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-workflow-{run_id}"
    workflow_name = f"workflow-{run_id}"

    stub.namespaces.Create(CreateNamespaceRequest(name=namespace, recursive=True))

    created = create_resource(
        stub,
        namespace,
        "Workflow",
        workflow_name,
        ResourceSpec(
            workflow=WorkflowSpec(
                description="E2E transform workflow",
                input_schema_json=json.dumps({
                    "type": "object",
                    "required": ["account"],
                    "properties": {"account": {"type": "string"}},
                }),
                output_schema_json=json.dumps({
                    "type": "object",
                    "required": ["summary", "score"],
                    "properties": {
                        "summary": {"type": "string"},
                        "score": {"type": "integer"},
                    },
                }),
                steps=[
                    WorkflowStep(
                        id="summarize",
                        type="transform",
                        input_json=json.dumps({
                            "summary": "${$.input.account} is healthy",
                            "score": 92,
                        }),
                    )
                ],
                output_json=json.dumps({
                    "summary": "${$.steps.summarize.output.summary}",
                    "score": "${$.steps.summarize.output.score}",
                }),
            ),
        ),
    )
    assert created.metadata.name == workflow_name

    run = stub.workflows.CreateRun(CreateWorkflowRunRequest(
        ns=namespace,
        workflow=workflow_name,
        input_json=json.dumps({"account": "acme"}),
        labels={"source": "pytest"},
    )).run
    assert run.id
    assert run.status == "QUEUED"

    completed = None
    for _ in range(30):
        time.sleep(1)
        response = stub.workflows.GetRun(GetWorkflowRunRequest(
            ns=namespace,
            workflow=workflow_name,
            run_id=run.id,
        ))
        if response.run.status == "COMPLETED":
            completed = response
            break

    assert completed is not None, "Workflow run did not complete through worker in time"
    assert completed.run.labels["source"] == "pytest"
    assert json.loads(completed.run.output_json) == {
        "summary": "acme is healthy",
        "score": 92,
    }
    assert len(completed.steps) == 1
    assert completed.steps[0].step_id == "summarize"
    assert completed.steps[0].status == "COMPLETED"

    listed = stub.workflows.ListRuns(ListWorkflowRunsRequest(
        ns=namespace,
        workflow=workflow_name,
        page_size=10,
    ))
    assert [item.id for item in listed.runs] == [run.id]
    assert listed.has_more is False

    events = list(stub.workflows.StreamEvents(StreamWorkflowEventsRequest(
        ns=namespace,
        workflow=workflow_name,
        run_id=run.id,
    ), timeout=10))
    event_types = [event.type for event in events]
    assert "run_started" in event_types
    assert "step_completed" in event_types
    assert event_types[-1] == "run_completed"

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
