import os
import sys
import time
import uuid

import grpc

# Important: Add generated protos to path so "proto.xxx" resolves locally and not to proto_plus
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))

from e2e import scenarios as e2e
from talon_client import (
    AppendSessionMessageRequest,
    CreateNamespaceRequest,
    CreateResourceRequest,
    CreateSessionRequest,
    DeleteSessionRequest,
    GetSearchResultRequest,
    TalonClient,
)
from talon_client.data import (
    ROLE_USER,
    SESSION_MESSAGE_PART_TYPE_TEXT,
    SessionMessage,
    SessionMessagePart,
)
from talon_client.resources import (
    AgentSpec,
    KnowledgeSpec,
    Model,
    ResourceManifest,
    ResourceMeta,
    ResourceSpec,
    WorkflowSpec,
)


def ensure_namespace(stub, name):
    try:
        stub.namespaces.Create(CreateNamespaceRequest(name=name, recursive=True))
    except grpc.RpcError as err:
        if err.code() != grpc.StatusCode.ALREADY_EXISTS:
            raise


def create_resource(stub, ns, kind, name, spec):
    return stub.resources.Create(
        CreateResourceRequest(
            ns=ns,
            manifest=ResourceManifest(
                api_version="talon.impalasys.com/v1",
                kind=kind,
                metadata=ResourceMeta(name=name, namespace=ns),
                spec=spec,
            ),
        )
    ).resource


def create_agent(stub, namespace, name):
    return create_resource(
        stub,
        namespace,
        "Agent",
        name,
        ResourceSpec(
            agent=AgentSpec(
                model_policy={
                    "profiles": [
                        {
                            "name": "default",
                            "model": Model(
                                provider="mock",
                                name="minimax/minimax-m2.7",
                                temperature=0.0,
                            ),
                        }
                    ]
                },
                system_prompt="Search E2E test agent.",
            )
        ),
    )


def append_user_message(stub, namespace, agent, session_id, text, token):
    now = int(time.time() * 1_000_000)
    message_id = f"msg-{token}"
    return stub.sessions.AppendMessage(
        AppendSessionMessageRequest(
            ns=namespace,
            agent=agent,
            session_id=session_id,
            message=SessionMessage(
                id=message_id,
                role=ROLE_USER,
                created_at=now,
                labels={"source": "search-e2e"},
                parts=[
                    SessionMessagePart(
                        id="000000",
                        part_type=SESSION_MESSAGE_PART_TYPE_TEXT,
                        content=text,
                        created_at=now,
                    )
                ],
            ),
        )
    ).message


def assert_session_message_document(result, namespace, agent, session_id, token):
    document = result.document
    assert document.source.namespace == namespace
    assert document.source.kind == "SessionMessage"
    assert document.document_kind == "part"
    assert document.attributes.get("part_type", "") == "TEXT"
    assert document.attributes.get("agent", "") == agent
    assert document.attributes.get("session_id", "") == session_id
    assert token in result.snippet


def test_postgres_pubsub_session_search_indexes_opens_and_deletes(
    gateway_channel,
    mock_llm_server,
):
    stub = TalonClient(gateway_channel)
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-search-{suffix}"
    agent = f"search-agent-{suffix}"
    token = f"sessiontoken{suffix}"
    message_text = f"Please remember {token} for session search."

    ensure_namespace(stub, namespace)
    create_agent(stub, namespace, agent)
    session_id = stub.sessions.Create(
        CreateSessionRequest(ns=namespace, agent=agent)
    ).session_id
    append_user_message(stub, namespace, agent, session_id, message_text, token)

    result, _ = e2e.wait_for_session_search(
        stub,
        namespace,
        agent,
        token,
        lambda item: item.document.attributes.get("session_id", "") == session_id
        and item.document.document_kind == "part",
        session_id=session_id,
    )
    assert_session_message_document(result, namespace, agent, session_id, token)

    opened = stub.searches.GetResult(
        GetSearchResultRequest(ns=namespace, document_id=result.document.id)
    )
    assert opened.document.ref.id == result.document.id
    assert message_text in opened.content

    deleted = stub.sessions.Delete(
        DeleteSessionRequest(ns=namespace, agent=agent, session_id=session_id)
    )
    assert deleted.success is True
    e2e.wait_for_no_session_search_results(
        stub,
        namespace,
        agent,
        token,
        session_id=session_id,
    )


def test_postgres_pubsub_workspace_search_indexes_resources_and_knowledge(
    gateway_channel,
    mock_llm_server,
):
    stub = TalonClient(gateway_channel)
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-workspace-search-{suffix}"
    agent = f"workspace-agent-{suffix}"
    workflow_name = f"workflow-{suffix}"
    knowledge_name = f"knowledge-{suffix}"
    workflow_token = f"workflowtoken{suffix}"
    knowledge_token = f"knowledgetoken{suffix}"

    ensure_namespace(stub, namespace)
    create_agent(stub, namespace, agent)
    create_resource(
        stub,
        namespace,
        "Workflow",
        workflow_name,
        ResourceSpec(
            workflow=WorkflowSpec(
                description=f"Workflow metadata should include {workflow_token}."
            )
        ),
    )

    workflow_result, _ = e2e.wait_for_search(
        stub,
        namespace,
        workflow_token,
        lambda item: item.document.source.kind == "Workflow"
        and item.document.document_kind == "metadata",
        resource_kinds=["Workflow"],
    )
    workflow_doc = workflow_result.document
    assert workflow_doc.source.namespace == namespace
    assert workflow_doc.source.kind == "Workflow"
    assert workflow_doc.document_kind == "metadata"
    assert workflow_token in workflow_result.snippet

    opened_workflow = stub.searches.GetResult(
        GetSearchResultRequest(ns=namespace, document_id=workflow_doc.id)
    )
    assert workflow_name in opened_workflow.content
    assert workflow_token in opened_workflow.content

    create_resource(
        stub,
        namespace,
        "Knowledge",
        knowledge_name,
        ResourceSpec(
            knowledge=KnowledgeSpec(
                path=f"{knowledge_name}.md",
                content=f"Knowledge content should include {knowledge_token}.",
            )
        ),
    )

    knowledge_result, _ = e2e.wait_for_search(
        stub,
        namespace,
        knowledge_token,
        lambda item: item.document.source.kind == "Knowledge"
        and item.document.document_kind == "content",
        resource_kinds=["Knowledge"],
    )
    knowledge_doc = knowledge_result.document
    assert knowledge_doc.source.namespace == namespace
    assert knowledge_doc.source.kind == "Knowledge"
    assert knowledge_doc.document_kind == "content"
    assert knowledge_token in knowledge_result.snippet

    opened_knowledge = stub.searches.GetResult(
        GetSearchResultRequest(ns=namespace, document_id=knowledge_doc.id)
    )
    assert knowledge_token in opened_knowledge.content


def test_sqlite_local_socket_session_search_indexes_and_opens(
    gateway_channel_sqlite,
    mock_llm_server,
):
    stub = TalonClient(gateway_channel_sqlite)
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-sqlite-search-{suffix}"
    agent = f"sqlite-search-agent-{suffix}"
    token = f"sqlitesessiontoken{suffix}"
    message_text = f"SQLite local search should find {token}."

    ensure_namespace(stub, namespace)
    create_agent(stub, namespace, agent)
    session_id = stub.sessions.Create(
        CreateSessionRequest(ns=namespace, agent=agent)
    ).session_id
    append_user_message(stub, namespace, agent, session_id, message_text, token)

    result, _ = e2e.wait_for_session_search(
        stub,
        namespace,
        agent,
        token,
        lambda item: item.document.attributes.get("session_id", "") == session_id
        and item.document.document_kind == "part",
        session_id=session_id,
    )
    assert_session_message_document(result, namespace, agent, session_id, token)

    opened = stub.searches.GetResult(
        GetSearchResultRequest(ns=namespace, document_id=result.document.id)
    )
    assert opened.document.ref.id == result.document.id
    assert message_text in opened.content
