import json
import time
import uuid

from e2e.blackbox import create_resource, ensure_namespace
from e2e.stack import E2EStack
from talon_client import (
    CreateWorkflowRunRequest,
    GetWorkflowRunRequest,
    ListWorkflowRunsRequest,
    StreamWorkflowEventsRequest,
    TalonClient,
)
from talon_client.resources import ResourceSpec, WorkflowSpec, WorkflowStep


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
