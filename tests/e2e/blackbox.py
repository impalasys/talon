import time
from collections.abc import Sequence
from typing import Any

import grpc

from talon_client import (
    AppendSessionMessageRequest,
    CreateNamespaceRequest,
    CreateResourceRequest,
    ListResourcesRequest,
    SearchResult,
    TalonClient,
)
from talon_client.data import (
    ROLE_USER,
    SESSION_MESSAGE_PART_TYPE_TEXT,
    SessionMessage,
    SessionMessagePart,
)
from talon_client.resources import AgentSpec, ResourceManifest, ResourceMeta, ResourceSpec


PART_TYPE_TEXT = 1


def message_text(message: SessionMessage) -> str:
    return "".join(part.content for part in message.parts if part.part_type == PART_TYPE_TEXT)


def assistant_messages(messages: Sequence[SessionMessage]) -> list[SessionMessage]:
    return [message for message in messages if message.role == 2]


def last_assistant_message(messages: Sequence[SessionMessage]) -> SessionMessage | None:
    assistants = assistant_messages(messages)
    return assistants[-1] if assistants else None


def ensure_namespace(stub: TalonClient, name: str) -> None:
    try:
        stub.namespaces.Create(CreateNamespaceRequest(name=name, recursive=True))
    except grpc.RpcError as err:
        if err.code() != grpc.StatusCode.ALREADY_EXISTS:
            raise


def create_resource(
    stub: TalonClient,
    ns: str,
    kind: str,
    name: str,
    spec: ResourceSpec,
) -> Any:
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


def create_agent_resource(
    stub: TalonClient,
    ns: str,
    name: str,
    spec: AgentSpec,
) -> Any:
    return create_resource(
        stub,
        ns,
        "Agent",
        name,
        ResourceSpec(agent=spec),
    )


def wait_for_worker_endpoint(
    stub: TalonClient,
    expected_url: str,
    attempts: int = 30,
    delay: float = 1,
) -> None:
    last_urls: list[str] = []
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


def assert_worker_registered(stub: TalonClient, expected_url: str) -> None:
    wait_for_worker_endpoint(stub, expected_url)


def append_user_message(
    stub: TalonClient,
    namespace: str,
    agent: str,
    session_id: str,
    text: str,
    token: str,
) -> SessionMessage:
    now = int(time.time() * 1_000_000)
    return stub.sessions.AppendMessage(
        AppendSessionMessageRequest(
            ns=namespace,
            agent=agent,
            session_id=session_id,
            message=SessionMessage(
                id=f"msg-{token}",
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


def assert_session_message_document(
    result: SearchResult,
    namespace: str,
    agent: str,
    session_id: str,
    token: str,
) -> None:
    document = result.document
    assert document.source.namespace == namespace
    assert document.source.kind == "SessionMessage"
    assert document.document_kind == "part"
    assert document.attributes.get("part_type", "") == "TEXT"
    assert document.attributes.get("agent", "") == agent
    assert document.attributes.get("session_id", "") == session_id
    assert token in result.snippet
