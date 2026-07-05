import os
import sys
import uuid

# Important: Add generated protos to path so "proto.xxx" resolves locally and not to proto_plus
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "generated")))

from e2e import scenarios as e2e
from e2e.blackbox import (
    append_user_message,
    assert_session_message_document,
    create_agent_resource,
    create_resource,
    ensure_namespace,
)
from e2e.stack import E2EStack
from talon_client import (
    CreateSessionRequest,
    DeleteSessionRequest,
    GetSearchResultRequest,
    TalonClient,
)
from talon_client.resources import AgentSpec, KnowledgeSpec, Model, ResourceSpec, WorkflowSpec


def test_session_search_indexes_opens_and_deletes(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    # Verify that session message search indexes a user message, can open the
    # indexed document content, and removes the document after session deletion.
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-search-{stack.name}-{suffix}"
    agent = f"search-agent-{suffix}"
    token = f"sessiontoken{suffix}"
    expected_message_text = f"Please remember {token} for session search."

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
                            name="minimax/minimax-m2.7",
                            temperature=0.0,
                        ),
                    }
                ]
            },
            system_prompt="Search E2E test agent.",
        ),
    )
    session_id = client.sessions.Create(
        CreateSessionRequest(ns=namespace, agent=agent)
    ).session_id
    append_user_message(client, namespace, agent, session_id, expected_message_text, token)

    result, _ = e2e.wait_for_session_search(
        client,
        namespace,
        agent,
        token,
        lambda item: item.document.attributes.get("session_id", "") == session_id
        and item.document.document_kind == "part",
        session_id=session_id,
    )
    assert_session_message_document(result, namespace, agent, session_id, token)

    opened = client.searches.GetResult(
        GetSearchResultRequest(ns=namespace, document_id=result.document.id)
    )
    assert opened.document.ref.id == result.document.id
    assert expected_message_text in opened.content

    deleted = client.sessions.Delete(
        DeleteSessionRequest(ns=namespace, agent=agent, session_id=session_id)
    )
    assert deleted.success is True
    e2e.wait_for_no_session_search_results(
        client,
        namespace,
        agent,
        token,
        session_id=session_id,
    )


def test_workspace_search_indexes_resources_and_knowledge(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    # Verify that workspace search indexes both workflow metadata and knowledge
    # content and returns the stored document bodies when results are opened.
    suffix = uuid.uuid4().hex[:8]
    namespace = f"talon-workspace-search-{stack.name}-{suffix}"
    agent = f"workspace-agent-{suffix}"
    workflow_name = f"workflow-{suffix}"
    knowledge_name = f"knowledge-{suffix}"
    workflow_token = f"workflowtoken{suffix}"
    knowledge_token = f"knowledgetoken{suffix}"

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
                            name="minimax/minimax-m2.7",
                            temperature=0.0,
                        ),
                    }
                ]
            },
            system_prompt="Search E2E test agent.",
        ),
    )
    create_resource(
        client,
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
        client,
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

    opened_workflow = client.searches.GetResult(
        GetSearchResultRequest(ns=namespace, document_id=workflow_doc.id)
    )
    assert workflow_name in opened_workflow.content
    assert workflow_token in opened_workflow.content

    create_resource(
        client,
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
        client,
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

    opened_knowledge = client.searches.GetResult(
        GetSearchResultRequest(ns=namespace, document_id=knowledge_doc.id)
    )
    assert knowledge_token in opened_knowledge.content
