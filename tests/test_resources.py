import time
import uuid

from e2e.blackbox import create_agent_resource, create_resource
from e2e.stack import E2EStack
from talon_client import (
    CreateNamespaceRequest,
    DeleteResourceRequest,
    GetKnowledgeRequest,
    GetResourceRequest,
    ListResourcesRequest,
    SearchKnowledgeRequest,
    TalonClient,
)
from talon_client.resources import (
    AgentSpec,
    KnowledgeSpec,
    Model,
    ResourceSpec,
    ScheduleSpec,
    ScheduleTarget,
)


def test_knowledge_crud_and_search(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    # Verify knowledge resources can be created, fetched, listed, searched, and
    # deleted through the public APIs on each supported stack.
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-knowledge-{run_id}"
    agent_name = f"knowledge-agent-{run_id}"

    client.namespaces.Create(CreateNamespaceRequest(name=namespace, recursive=True))

    agent_spec = AgentSpec(
        model_policy={
            "profiles": [
                {
                    "name": "default",
                    "model": Model(provider="mock", name="minimax", temperature=0.7),
                }
            ]
        },
        system_prompt="Knowledge test agent.",
    )
    create_agent_resource(client, namespace, agent_name, agent_spec)

    created = create_resource(
        client,
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

    fetched = client.resources.Get(
        GetResourceRequest(
            ns=namespace,
            kind="Knowledge",
            name="guide",
        )
    )
    assert fetched.resource.spec.knowledge.path == "guide.md"
    assert "runtime facts" in fetched.resource.spec.knowledge.content

    listed = client.resources.List(ListResourcesRequest(ns=namespace, kind="Knowledge"))
    assert len(listed.resources) == 1
    assert listed.resources[0].metadata.name == "guide"

    modules = client.knowledge.Get(
        GetKnowledgeRequest(
            ns=namespace,
            agent=agent_name,
            path="guide.md",
        )
    )
    assert len(modules.modules) == 1
    assert modules.modules[0].path == "guide.md"
    assert "guide documents" in modules.modules[0].content

    search = None
    for _ in range(30):
        search = client.knowledge.Search(
            SearchKnowledgeRequest(
                ns=namespace,
                agent=agent_name,
                query="runtime facts",
                limit=10,
            ),
            timeout=10,
        )
        if search.search_results:
            break
        time.sleep(1)

    assert search is not None
    assert len(search.results) >= 1
    assert search.results[0].path == "guide.md"
    assert len(search.search_results) >= 1
    assert search.search_results[0].document.source.kind == "Knowledge"

    deleted = client.resources.Delete(
        DeleteResourceRequest(
            ns=namespace,
            kind="Knowledge",
            name="guide",
        )
    )
    assert deleted.success is True


def test_schedule_crud_round_trip(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    # Verify schedules can be created, fetched, listed, and deleted through the
    # resource APIs on each supported stack.
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-schedule-{run_id}"
    agent_name = f"schedule-agent-{run_id}"
    schedule_name = f"schedule-{run_id}"

    client.namespaces.Create(CreateNamespaceRequest(name=namespace, recursive=True))

    agent_spec = AgentSpec(
        model_policy={
            "profiles": [
                {
                    "name": "default",
                    "model": Model(provider="mock", name="minimax", temperature=0.7),
                }
            ]
        },
        system_prompt="Schedule test agent.",
    )
    create_agent_resource(client, namespace, agent_name, agent_spec)

    created = create_resource(
        client,
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

    fetched = client.resources.Get(GetResourceRequest(ns=namespace, kind="Schedule", name=schedule_name))
    assert fetched.resource.metadata.name == schedule_name
    assert fetched.resource.spec.schedule.target.agent == agent_name

    listed = client.resources.List(ListResourcesRequest(ns=namespace, kind="Schedule"))
    assert len(listed.resources) == 1
    assert listed.resources[0].metadata.name == schedule_name

    deleted = client.resources.Delete(DeleteResourceRequest(ns=namespace, kind="Schedule", name=schedule_name))
    assert deleted.success is True
