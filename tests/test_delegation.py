import json
import time
import uuid

import grpc
from google.protobuf.struct_pb2 import ListValue, Value

from e2e.blackbox import (
    create_agent_resource,
    ensure_namespace,
    last_assistant_message,
    message_text,
)
from e2e.stack import E2EStack
from talon_client import (
    CreateSessionRequest,
    GetResourceRequest,
    GetSessionRequest,
    ListResourcesRequest,
    ListSessionsRequest,
    SendMessageRequest,
    TalonClient,
)
from talon_client.resources import AgentSpec, Model
from talon_client.resources import A2A, Connection, ConnectionRef, InternalConnectionRef


PART_TYPE_TOOL_RESULT = 4


def _send_message_when_available(
    client: TalonClient,
    request: SendMessageRequest,
    attempts: int = 15,
    delay: float = 1.0,
) -> None:
    for attempt in range(attempts):
        try:
            client.sessions.SendMessage(request)
            return
        except grpc.RpcError as err:
            if (
                err.code() != grpc.StatusCode.RESOURCE_EXHAUSTED
                or "currently generating" not in err.details()
                or attempt == attempts - 1
            ):
                raise
            time.sleep(delay)


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
    _send_message_when_available(
        client,
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
    assert reviewed_task.status.task.output_artifact_uris
    artifact_uri = reviewed_task.status.task.output_artifact_uris[0]
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

    _send_message_when_available(
        client,
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
    _send_message_when_available(
        client,
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
    assert reviewed_task.status.task.output_artifact_uris
    artifact_uri = reviewed_task.status.task.output_artifact_uris[0]
    assert artifact_uri.startswith(
        f"artifact://{reviewer_namespace}/legal-reviewer-agent/"
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

    _send_message_when_available(
        client,
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


def test_delegated_final_text_artifact_tag_becomes_readable_task_output(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    namespace_suffix = f"{stack.name}-{uuid.uuid4().hex[:8]}"
    coordinator_namespace = f"talon-inline-coordinator-{namespace_suffix}"
    writer_namespace = f"talon-inline-writer-{namespace_suffix}"
    ensure_namespace(client, coordinator_namespace)
    ensure_namespace(client, writer_namespace)
    create_agent_resource(
        client,
        writer_namespace,
        "inline-writer-agent",
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
                "You are an inline artifact writer. Return final markdown "
                "artifacts with <artifact> tags when assigned a Talon Task."
            ),
        ),
    )
    create_agent_resource(
        client,
        coordinator_namespace,
        "inline-coordinator-agent",
        AgentSpec(
            capabilities={
                "tasks": _capability_values("create", "inspect"),
                "sessions": _capability_values("read:messages"),
            },
            a2a=_internal_a2a(
                "inline-writer",
                writer_namespace,
                "inline-writer-agent",
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
                "You are an inline artifact coordinator. Delegate drafting work "
                "with delegate_task, then inspect delegated artifacts."
            ),
        ),
    )

    coordinator_session_id = client.sessions.Create(
        CreateSessionRequest(
            agent="inline-coordinator-agent",
            ns=coordinator_namespace,
        )
    ).session_id
    _send_message_when_available(
        client,
        SendMessageRequest(
            agent="inline-coordinator-agent",
            session_id=coordinator_session_id,
            ns=coordinator_namespace,
            message=(
                "Please delegate inline artifact drafting task to the "
                "inline-writer connection."
            ),
        )
    )

    task = None
    for _ in range(60):
        time.sleep(1)
        task_resources = list(
            client.resources.List(
                ListResourcesRequest(ns=coordinator_namespace, kind="Task")
            ).resources
        )
        if not task_resources:
            continue
        task = client.resources.Get(
            GetResourceRequest(
                ns=coordinator_namespace,
                kind="Task",
                name=task_resources[0].metadata.name,
            )
        ).resource
        if task.status.task.phase == 4 and task.status.task.output_artifact_uris:
            break
    assert task is not None, "coordinator did not create inline artifact task"
    assert task.status.task.phase == 4
    assert task.status.task.output_artifact_uris
    artifact_uri = task.status.task.output_artifact_uris[0]
    assert artifact_uri.startswith(f"artifact://{writer_namespace}/inline-writer-agent/")

    child_session_id = task.status.task.execution_ref.session_id
    assert child_session_id
    writer = client.sessions.Get(
        GetSessionRequest(
            agent="inline-writer-agent",
            session_id=child_session_id,
            ns=writer_namespace,
        )
    )
    writer_text = "\n".join(message_text(message) for message in writer.messages)
    assert "<artifact" not in writer_text
    assert artifact_uri in writer_text
    writer_tool_results = [
        part
        for message in writer.messages
        for part in message.parts
        if part.part_type == PART_TYPE_TOOL_RESULT
    ]
    assert not writer_tool_results, "writer should not need create_artifact"

    for _ in range(45):
        time.sleep(1)
        coordinator = client.sessions.Get(
            GetSessionRequest(
                agent="inline-coordinator-agent",
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
        raise AssertionError("coordinator was not woken with inline artifact URI")

    _send_message_when_available(
        client,
        SendMessageRequest(
            agent="inline-coordinator-agent",
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
                agent="inline-coordinator-agent",
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
            "Directors should approve" in output.get("content", "")
            for output in read_outputs
        ):
            break

    assert any(
        "Directors should approve the operating plan" in output.get("content", "")
        for output in read_outputs
    ), "coordinator could not read the inline delegated artifact"


def test_nested_policy_document_delegation_forwards_artifact_uri(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    namespace_suffix = f"{stack.name}-{uuid.uuid4().hex[:8]}"
    coordinator_namespace = f"talon-policy-coordinator-{namespace_suffix}"
    editor_namespace = f"talon-policy-editor-{namespace_suffix}"
    drafter_namespace = f"talon-policy-drafter-{namespace_suffix}"
    ensure_namespace(client, coordinator_namespace)
    ensure_namespace(client, editor_namespace)
    ensure_namespace(client, drafter_namespace)

    create_agent_resource(
        client,
        drafter_namespace,
        "policy-drafter-agent",
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
            system_prompt="You are a policy drafter. Produce markdown artifacts.",
        ),
    )
    create_agent_resource(
        client,
        editor_namespace,
        "policy-editor-agent",
        AgentSpec(
            capabilities={"tasks": _capability_values("create", "inspect")},
            a2a=_internal_a2a(
                "policy-drafter",
                drafter_namespace,
                "policy-drafter-agent",
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
                "You are a policy editor. Delegate drafting work and forward "
                "review-ready artifact URIs to your owner task."
            ),
        ),
    )
    create_agent_resource(
        client,
        coordinator_namespace,
        "policy-coordinator-agent",
        AgentSpec(
            capabilities={
                "tasks": _capability_values("create", "inspect"),
                "sessions": _capability_values("read:messages"),
            },
            a2a=_internal_a2a(
                "policy-editor",
                editor_namespace,
                "policy-editor-agent",
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
                "You are a policy coordinator. Delegate policy refinement work "
                "with delegate_task, then inspect delegated artifacts."
            ),
        ),
    )

    coordinator_session_id = client.sessions.Create(
        CreateSessionRequest(
            agent="policy-coordinator-agent",
            ns=coordinator_namespace,
        )
    ).session_id
    _send_message_when_available(
        client,
        SendMessageRequest(
            agent="policy-coordinator-agent",
            session_id=coordinator_session_id,
            ns=coordinator_namespace,
            message=(
                "Please delegate policy document refinement task to the "
                "policy-editor connection."
            ),
        )
    )

    parent_task = None
    for _ in range(60):
        time.sleep(1)
        parent_tasks = list(
            client.resources.List(
                ListResourcesRequest(ns=coordinator_namespace, kind="Task")
            ).resources
        )
        if parent_tasks:
            parent_task = client.resources.Get(
                GetResourceRequest(
                    ns=coordinator_namespace,
                    kind="Task",
                    name=parent_tasks[0].metadata.name,
                )
            ).resource
            if (
                parent_task.status.task.phase == 4
                and parent_task.status.task.output_artifact_uris
            ):
                break
    assert parent_task is not None, "coordinator did not create parent task"
    assert parent_task.status.task.phase == 4
    assert parent_task.status.task.output_artifact_uris
    artifact_uri = parent_task.status.task.output_artifact_uris[0]
    assert artifact_uri.startswith(
        f"artifact://{drafter_namespace}/policy-drafter-agent/"
    )

    editor_tasks = list(
        client.resources.List(
            ListResourcesRequest(ns=editor_namespace, kind="Task")
        ).resources
    )
    assert len(editor_tasks) == 1
    child_task = client.resources.Get(
        GetResourceRequest(
            ns=editor_namespace,
            kind="Task",
            name=editor_tasks[0].metadata.name,
        )
    ).resource
    assert child_task.status.task.phase == 4
    assert child_task.status.task.output_artifact_uris == [artifact_uri]
