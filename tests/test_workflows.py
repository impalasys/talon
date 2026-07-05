import json
import threading
import time
import uuid

from e2e.blackbox import create_resource, ensure_namespace
from e2e.stack import E2EStack
from talon_client import (
    CreateNamespaceRequest,
    CreateWorkflowRunRequest,
    GetResourceRequest,
    GetWorkflowRunRequest,
    ListWorkflowRunsRequest,
    StreamWorkflowEventsRequest,
    TalonClient,
)
from talon_client.resources import (
    DeploymentPlacement,
    DeploymentSpec,
    NamespaceSelector,
    ResourceMeta,
    ResourceSpec,
    TemplateSpec,
    WorkflowSpec,
    WorkflowStep,
)


def test_workflow_transform_run_completes_through_worker(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    # Verify a simple transform workflow is created, dispatched through the
    # worker, reaches COMPLETED, and emits the expected run events.
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-workflow-{stack.name}-{run_id}"
    workflow_name = f"workflow-{run_id}"

    ensure_namespace(client, namespace)
    created = create_resource(
        client,
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

    run = client.workflows.CreateRun(
        CreateWorkflowRunRequest(
            ns=namespace,
            workflow=workflow_name,
            input_json=json.dumps({"account": "acme"}),
            labels={"source": "pytest"},
        )
    ).run
    assert run.id
    assert run.status == "QUEUED"

    completed = None
    for _ in range(30):
        time.sleep(1)
        response = client.workflows.GetRun(
            GetWorkflowRunRequest(
                ns=namespace,
                workflow=workflow_name,
                run_id=run.id,
            )
        )
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

    listed = client.workflows.ListRuns(
        ListWorkflowRunsRequest(
            ns=namespace,
            workflow=workflow_name,
            page_size=10,
        )
    )
    assert [item.id for item in listed.runs] == [run.id]
    assert listed.has_more is False

    events = list(
        client.workflows.StreamEvents(
            StreamWorkflowEventsRequest(
                ns=namespace,
                workflow=workflow_name,
                run_id=run.id,
            ),
            timeout=10,
        )
    )
    event_types = [event.type for event in events]
    assert "run_started" in event_types
    assert "step_completed" in event_types
    assert event_types[-1] == "run_completed"


def test_deployment_materialized_workflow_runs_in_target_namespace(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    # Verify the Conic-style deployment model: a Workflow Template in a parent
    # namespace is materialized as a real Workflow resource in each selected
    # child namespace, and WorkflowService.CreateRun uses that child resource.
    run_id = uuid.uuid4().hex[:8]
    parent_namespace = f"talon-workflow-deploy-{stack.name}-{run_id}"
    target_namespace = f"{parent_namespace}:customer"
    workflow_name = f"deployed-workflow-{run_id}"
    template_name = f"workflow-template-{run_id}"
    deployment_name = f"workflow-deployment-{run_id}"

    client.namespaces.Create(
        CreateNamespaceRequest(name=parent_namespace, recursive=True)
    )
    client.namespaces.Create(
        CreateNamespaceRequest(
            name=target_namespace,
            recursive=True,
            labels={"app.conic/customer-workspace": "true"},
        )
    )

    create_resource(
        client,
        parent_namespace,
        "Template",
        template_name,
        ResourceSpec(
            template=TemplateSpec(
                kind="Workflow",
                metadata=ResourceMeta(
                    name=workflow_name,
                    labels={"app.conic/kind": "backlink-outreach"},
                ),
                spec_json=json.dumps({
                    "description": "Prepare outreach drafts for {{ namespace.name }}.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["account"],
                        "properties": {"account": {"type": "string"}},
                    },
                    "outputSchema": {
                        "type": "object",
                        "required": ["message"],
                        "properties": {"message": {"type": "string"}},
                    },
                    "steps": [
                        {
                            "id": "copy",
                            "type": "transform",
                            "input": {
                                "message": "hello ${$.input.account}",
                            },
                        }
                    ],
                    "output": {
                        "message": "${$.steps.copy.output.message}",
                    },
                }),
            )
        ),
    )
    create_resource(
        client,
        parent_namespace,
        "Deployment",
        deployment_name,
        ResourceSpec(
            deployment=DeploymentSpec(
                placement=DeploymentPlacement(
                    namespace_selector=NamespaceSelector(
                        parent=parent_namespace,
                        match_labels={"app.conic/customer-workspace": "true"},
                    )
                ),
                templates=[template_name],
            )
        ),
    )

    rendered = None
    for _ in range(30):
        time.sleep(1)
        try:
            rendered = client.resources.Get(
                GetResourceRequest(
                    ns=target_namespace,
                    kind="Workflow",
                    name=workflow_name,
                ),
                timeout=10,
            ).resource
            break
        except Exception:
            continue

    assert rendered is not None, "Deployment did not materialize workflow"
    assert rendered.metadata.namespace == target_namespace
    assert rendered.metadata.name == workflow_name
    assert rendered.metadata.labels["app.conic/kind"] == "backlink-outreach"
    assert rendered.spec.workflow.description == (
        f"Prepare outreach drafts for {target_namespace}."
    )

    run = client.workflows.CreateRun(
        CreateWorkflowRunRequest(
            ns=target_namespace,
            workflow=workflow_name,
            input_json=json.dumps({"account": "acme"}),
            labels={"source": "deployment-e2e"},
        )
    ).run
    assert run.id
    assert run.ns == target_namespace
    assert run.workflow == workflow_name

    streamed_events = []
    stream_errors = []

    def collect_events() -> None:
        try:
            for event in client.workflows.StreamEvents(
                StreamWorkflowEventsRequest(
                    ns=target_namespace,
                    workflow=workflow_name,
                    run_id=run.id,
                ),
                timeout=30,
            ):
                streamed_events.append(event)
                if event.type in {"run_completed", "run_failed", "run_cancelled"}:
                    break
        except Exception as exc:
            stream_errors.append(exc)

    stream_thread = threading.Thread(target=collect_events, daemon=True)
    stream_thread.start()

    completed = None
    for _ in range(30):
        time.sleep(1)
        response = client.workflows.GetRun(
            GetWorkflowRunRequest(
                ns=target_namespace,
                workflow=workflow_name,
                run_id=run.id,
            )
        )
        if response.run.status == "COMPLETED":
            completed = response
            break

    assert completed is not None, "Materialized workflow run did not complete"
    assert json.loads(completed.run.output_json) == {"message": "hello acme"}

    stream_thread.join(timeout=10)
    assert not stream_thread.is_alive(), "Workflow event stream did not terminate"
    assert stream_errors == []
    event_types = [event.type for event in streamed_events]
    assert "run_started" in event_types
    assert "step_completed" in event_types
    assert event_types[-1] == "run_completed"
