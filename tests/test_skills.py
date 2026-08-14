import json
import time
import uuid

import grpc
import pytest
import requests

from e2e.blackbox import create_agent_resource, last_assistant_message, message_text
from e2e.stack import MOCK_LLM_PORT

from talon_client import (
    CreateFileRequest,
    CreateNamespaceRequest,
    CreateResourceRequest,
    CreateSessionRequest,
    FileRef,
    GetSessionRequest,
    GetResourceRequest,
    ListFilesRequest,
    ListResourcesRequest,
    ReadFileRequest,
    SendMessageRequest,
    TalonClient,
)
from talon_client.resources import (
    FILE_INDEX_POLICY_NONE,
    FILE_PURPOSE_ARTIFACT,
    FILE_PURPOSE_SKILL,
    FILE_RETENTION_RETAINED,
    AgentSpec,
    Model,
    ResourceManifest,
    ResourceMeta,
    ResourceSpec,
    SkillSpec,
)


def _mock_control(method: str, path: str) -> dict:
    response = requests.request(
        method,
        f"http://127.0.0.1:{MOCK_LLM_PORT}{path}",
        timeout=5,
    )
    response.raise_for_status()
    return response.json()


def _wait_for_turn(
    client: TalonClient,
    namespace: str,
    agent: str,
    session_id: str,
    expected: str,
) -> None:
    for _ in range(30):
        time.sleep(1)
        session = client.sessions.Get(
            GetSessionRequest(ns=namespace, agent=agent, session_id=session_id)
        )
        assistant = last_assistant_message(session.messages)
        if (
            session.state == "IDLE"
            and assistant is not None
            and expected in message_text(assistant)
        ):
            return
    raise AssertionError(f"session {session_id} did not complete the expected turn")


def test_skill_package_create_list_read_and_path_guard(
    client: TalonClient,
) -> None:
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-skill-{run_id}"
    entrypoint = "/skills/review/SKILL.md"
    instructions = b"# Review\nCheck the diff before proposing changes.\n"

    client.namespaces.Create(CreateNamespaceRequest(name=namespace, recursive=True))
    created_skill = client.resources.Create(
        CreateResourceRequest(
            ns=namespace,
            manifest=ResourceManifest(
                api_version="talon.impalasys.com/v1",
                kind="Skill",
                metadata=ResourceMeta(name="review", namespace=namespace),
                spec=ResourceSpec(skill=SkillSpec(description="Review code")),
            ),
        )
    ).resource
    assert created_skill.spec.skill.description == "Review code"

    created_file = client.files.CreateFile(
        CreateFileRequest(
            namespace=namespace,
            path=entrypoint,
            media_type="text/markdown",
            purpose=FILE_PURPOSE_SKILL,
            index_policy=FILE_INDEX_POLICY_NONE,
            retention=FILE_RETENTION_RETAINED,
            content=instructions,
        )
    ).file
    assert created_file.spec.path == entrypoint
    assert created_file.spec.purpose == FILE_PURPOSE_SKILL

    listed_skills = client.resources.List(
        ListResourcesRequest(ns=namespace, kind="Skill")
    )
    assert [resource.metadata.name for resource in listed_skills.resources] == ["review"]

    fetched_skill = client.resources.Get(
        GetResourceRequest(ns=namespace, kind="Skill", name="review")
    ).resource
    assert fetched_skill.spec.skill.description == "Review code"

    listed_files = client.files.ListFiles(
        ListFilesRequest(namespace=namespace, prefix="/skills/review")
    )
    assert [file.spec.path for file in listed_files.files] == [entrypoint]
    assert listed_files.files[0].spec.purpose == FILE_PURPOSE_SKILL

    read_file = client.files.ReadFile(
        ReadFileRequest(file=FileRef(namespace=namespace, path=entrypoint))
    )
    assert read_file.content == instructions

    with pytest.raises(grpc.RpcError) as error:
        client.files.CreateFile(
            CreateFileRequest(
                namespace=namespace,
                path="/skills/review/README.md",
                media_type="text/markdown",
                purpose=FILE_PURPOSE_ARTIFACT,
                index_policy=FILE_INDEX_POLICY_NONE,
                retention=FILE_RETENTION_RETAINED,
                content=b"This must be a SKILL File.",
            )
        )
    assert error.value.code() == grpc.StatusCode.INVALID_ARGUMENT


def test_skill_activation_and_deactivation_are_sticky_per_session(
    stack,
    client: TalonClient,
) -> None:
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-skill-session-{run_id}"
    agent_name = f"skill-agent-{run_id}"
    entrypoint = "/skills/review/SKILL.md"

    client.namespaces.Create(CreateNamespaceRequest(name=namespace, recursive=True))
    create_agent_resource(
        client,
        namespace,
        agent_name,
        AgentSpec(
            model_policy={
                "profiles": [
                    {
                        "name": "default",
                        "model": Model(
                            provider="mock",
                            name="minimax-m2.7",
                            temperature=0.0,
                        ),
                    }
                ]
            },
            system_prompt="Use the available skills when the user asks to activate one.",
        ),
    )
    client.resources.Create(
        CreateResourceRequest(
            ns=namespace,
            manifest=ResourceManifest(
                api_version="talon.impalasys.com/v1",
                kind="Skill",
                metadata=ResourceMeta(name="review", namespace=namespace),
                spec=ResourceSpec(skill=SkillSpec(description="Review code")),
            ),
        )
    )
    client.files.CreateFile(
        CreateFileRequest(
            namespace=namespace,
            path=entrypoint,
            media_type="text/markdown",
            purpose=FILE_PURPOSE_SKILL,
            index_policy=FILE_INDEX_POLICY_NONE,
            retention=FILE_RETENTION_RETAINED,
            content=b"Review guidance: inspect every changed line.",
        )
    )

    _mock_control("POST", "/__control/reset")
    session_id = client.sessions.Create(
        CreateSessionRequest(ns=namespace, agent=agent_name)
    ).session_id
    client.sessions.SendMessage(
        SendMessageRequest(
            ns=namespace,
            agent=agent_name,
            session_id=session_id,
            message="please activate the review skill",
        )
    )
    _wait_for_turn(client, namespace, agent_name, session_id, "review skill is active")

    activated_state = _mock_control("GET", "/__control/state")
    activation_requests = activated_state["chat_requests"]
    assert any(
        tool_name == "activate_skill"
        for request in activation_requests
        for tool_name in request["toolNames"]
    )
    assert any(
        message.get("role") == "user"
        and "# ACTIVE SKILL: review" in json.dumps(message)
        for request in activation_requests
        for message in request["messages"]
    )
    assert any(
        message.get("role") == "system"
        and "Review code" in json.dumps(message)
        for request in activation_requests
        for message in request["messages"]
    )

    # Reset only the mock's request log; the Talon session's active-skill state
    # must survive and be included in the next turn.
    _mock_control("POST", "/__control/reset")
    client.sessions.SendMessage(
        SendMessageRequest(
            ns=namespace,
            agent=agent_name,
            session_id=session_id,
            message="please deactivate the review skill",
        )
    )
    _wait_for_turn(client, namespace, agent_name, session_id, "review skill is inactive")

    deactivated_state = _mock_control("GET", "/__control/state")
    deactivation_requests = deactivated_state["chat_requests"]
    assert any(
        tool_name == "deactivate_skill"
        for request in deactivation_requests
        for tool_name in request["toolNames"]
    )
    active_context_by_request = [
        any(
            message.get("role") == "user"
            and "# ACTIVE SKILL: review" in json.dumps(message)
            for message in request["messages"]
        )
        for request in deactivation_requests
    ]
    assert active_context_by_request[0] is True
    assert active_context_by_request[-1] is False
