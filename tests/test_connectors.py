import threading
import time
import uuid
from typing import Any

import uvicorn
from fastapi import FastAPI, Request

from e2e.blackbox import create_agent_resource, create_resource, ensure_namespace, message_text
from e2e.stack import E2EStack, unused_tcp_port
from talon_client import (
    GetResourceRequest,
    ListSessionMessagesRequest,
    ListSessionsRequest,
    TalonClient,
    UpdateSessionMessageRequest,
)
from talon_client.data import (
    ROLE_ASSISTANT,
    SESSION_MESSAGE_PART_TYPE_TEXT,
    MessageConsumer,
    Principal,
    ResourceRef as DataResourceRef,
    SessionMessageConsumer,
    SessionMessagePart,
)
from talon_client.proto.external.connectors_pb2 import (
    CONNECTOR_MESSAGE_EVENT_KIND_CREATED,
    CONNECTOR_MESSAGE_EVENT_STATUS_ACCEPTED,
    ConnectorMessageEvent,
)
from talon_client.resources import (
    AgentSpec,
    ConnectorClassAuthSpec,
    ConnectorClassRuntimeSpec,
    ConnectorClassSpec,
    ConnectorMatchIndex,
    ConnectorSecretRef,
    ConnectorSpec,
    Model,
    ResourceRef,
    ResourceSpec,
)


LABEL_CONNECTOR_DELIVERY_STATUS = "talon.impalasys.com/connector-delivery-status"
CONNECTOR_DELIVERY_PENDING_REVIEW = "pending_review"
CONNECTOR_DELIVERY_REQUESTED = "delivery_requested"
CONNECTOR_DELIVERY_DELIVERED = "delivered"


class MockConnectorRuntime:
    def __init__(self) -> None:
        self.port = unused_tcp_port()
        self.base_url = f"http://127.0.0.1:{self.port}"
        self.registrations: list[dict[str, Any]] = []
        self.deliveries: list[dict[str, Any]] = []
        self._app = FastAPI()
        self._server: uvicorn.Server | None = None
        self._thread: threading.Thread | None = None

        @self._app.post("/v1/clusters/register")
        async def register(request: Request) -> dict[str, Any]:
            payload = await request.json()
            self.registrations.append(payload)
            return {
                "registrationId": payload.get("registrationId")
                or payload.get("registration_id", "")
            }

        @self._app.post("/v1/deliveries")
        async def deliver(request: Request) -> dict[str, Any]:
            payload = await request.json()
            self.deliveries.append(payload)
            return {"accepted": True, "disposition": "accepted", "error": ""}

        @self._app.post("/v1/activities")
        async def activity() -> dict[str, Any]:
            return {"accepted": True, "disposition": "accepted", "error": ""}

    def start(self) -> "MockConnectorRuntime":
        self._server = uvicorn.Server(
            uvicorn.Config(
                self._app,
                host="127.0.0.1",
                port=self.port,
                log_level="warning",
            )
        )
        self._thread = threading.Thread(target=self._server.run, daemon=True)
        self._thread.start()
        deadline = time.time() + 10
        while time.time() < deadline:
            if self._server.started:
                return self
            time.sleep(0.05)
        raise RuntimeError("mock connector runtime failed to start")

    def stop(self) -> None:
        if self._server is not None:
            self._server.should_exit = True
        if self._thread is not None:
            self._thread.join(timeout=5)


def wait_for_connector_class_ready(
    client: TalonClient,
    namespace: str,
    name: str,
    runtime: MockConnectorRuntime,
) -> None:
    deadline = time.time() + 30
    last_phase = ""
    while time.time() < deadline:
        res = client.resources.Get(
            GetResourceRequest(ns=namespace, kind="ConnectorClass", name=name)
        )
        last_phase = res.resource.status.connector_class.phase
        if last_phase == "Ready" and runtime.registrations:
            return
        time.sleep(0.5)
    raise AssertionError(f"ConnectorClass did not become Ready; last phase={last_phase!r}")


def wait_for_connector_ready(client: TalonClient, namespace: str, name: str) -> None:
    deadline = time.time() + 30
    last_phase = ""
    while time.time() < deadline:
        res = client.resources.Get(GetResourceRequest(ns=namespace, kind="Connector", name=name))
        last_phase = res.resource.status.connector.phase
        if last_phase == "Ready":
            return
        time.sleep(0.5)
    raise AssertionError(f"Connector did not become Ready; last phase={last_phase!r}")


def wait_for_pending_assistant_reply(
    client: TalonClient,
    namespace: str,
    agent: str,
    session_id: str,
) -> Any:
    deadline = time.time() + 45
    last_messages = []
    while time.time() < deadline:
        response = client.sessions.ListMessages(
            ListSessionMessagesRequest(
                ns=namespace,
                agent=agent,
                session_id=session_id,
                page_size=50,
            )
        )
        last_messages = [item.message for item in response.items]
        for message in reversed(last_messages):
            if (
                message.role == ROLE_ASSISTANT
                and message.labels.get(LABEL_CONNECTOR_DELIVERY_STATUS)
                == CONNECTOR_DELIVERY_PENDING_REVIEW
            ):
                return message
        time.sleep(1)
    raise AssertionError(f"Timed out waiting for pending assistant reply; saw {last_messages!r}")


def wait_for_delivery_status(
    client: TalonClient,
    namespace: str,
    agent: str,
    session_id: str,
    message_id: str,
    status: str,
) -> Any:
    deadline = time.time() + 20
    while time.time() < deadline:
        response = client.sessions.ListMessages(
            ListSessionMessagesRequest(
                ns=namespace,
                agent=agent,
                session_id=session_id,
                page_size=50,
            )
        )
        for item in response.items:
            message = item.message
            if (
                message.id == message_id
                and message.labels.get(LABEL_CONNECTOR_DELIVERY_STATUS) == status
            ):
                return message
        time.sleep(0.5)
    raise AssertionError(f"Timed out waiting for delivery status {status!r}")


def latest_session_id(client: TalonClient, namespace: str, agent: str) -> str:
    response = client.sessions.List(ListSessionsRequest(ns=namespace, agent=agent))
    assert response.sessions, "connector event should create a session"
    return response.sessions[-1].session_id


def test_connector_hold_for_review_can_edit_then_deliver(
    talon_infrastructure_sqlite: E2EStack,
    gateway_channel_sqlite,
) -> None:
    runtime = MockConnectorRuntime().start()
    client = TalonClient(gateway_channel_sqlite)
    namespace = f"talon-connector-{uuid.uuid4().hex[:8]}"
    agent_name = "connector-agent"
    class_name = "mock-chat"
    connector_name = "room-one"
    registration_id = f"Namespace/{namespace}/ConnectorClass/{class_name}"
    try:
        ensure_namespace(client, namespace)
        create_agent_resource(
            client,
            namespace,
            agent_name,
            AgentSpec(
                model_policy={
                    "profiles": [
                        {
                            "name": "default",
                            "model": Model(provider="mock", name="minimax-m2.7"),
                        }
                    ]
                },
                system_prompt="Reply briefly to connector messages.",
            ),
        )
        create_resource(
            client,
            namespace,
            "ConnectorClass",
            class_name,
            ResourceSpec(
                connector_class=ConnectorClassSpec(
                    platform="mock",
                    runtime=ConnectorClassRuntimeSpec(kind="http", endpoint=runtime.base_url),
                    auth=ConnectorClassAuthSpec(
                        kind="apiKey",
                        api_key=ConnectorSecretRef(plain="mock-secret"),
                    ),
                    match_indexes=[
                        ConnectorMatchIndex(name="room", fields=["roomId"]),
                    ],
                )
            ),
        )
        wait_for_connector_class_ready(client, namespace, class_name, runtime)

        create_resource(
            client,
            namespace,
            "Connector",
            connector_name,
            ResourceSpec(
                connector=ConnectorSpec(
                    class_ref=ResourceRef(name=class_name),
                    enabled=True,
                    match_fields={"roomId": "room-1"},
                    consumer=MessageConsumer(
                        session=SessionMessageConsumer(
                            agent=DataResourceRef(name=agent_name),
                            continuity="reuse",
                            reply_mode="hold_for_review",
                        )
                    ),
                )
            ),
        )
        wait_for_connector_ready(client, namespace, connector_name)

        response = client.connectors.IngestMessageEvent(
            ConnectorMessageEvent(
                event_id=f"evt-{uuid.uuid4().hex}",
                event_kind=CONNECTOR_MESSAGE_EVENT_KIND_CREATED,
                registration_id=registration_id,
                connector_class=class_name,
                match_fields={"roomId": "room-1"},
                external_conversation_id="room-1",
                external_message_id="msg-1",
                conversation_type="room",
                sender=Principal(external_id="operator-1", display_name="Operator", kind="user"),
                text="hello from connector",
                event_time_ms=int(time.time() * 1000),
            ),
            timeout=10,
        )
        assert response.status == CONNECTOR_MESSAGE_EVENT_STATUS_ACCEPTED
        assert response.connector_name == connector_name

        session_id = latest_session_id(client, namespace, agent_name)
        pending = wait_for_pending_assistant_reply(client, namespace, agent_name, session_id)
        assert runtime.deliveries == []

        labels = dict(pending.labels)
        labels[LABEL_CONNECTOR_DELIVERY_STATUS] = CONNECTOR_DELIVERY_REQUESTED
        client.sessions.UpdateMessage(
            UpdateSessionMessageRequest(
                ns=namespace,
                agent=agent_name,
                session_id=session_id,
                message_id=pending.id,
                labels=labels,
                parts=[
                    SessionMessagePart(
                        part_type=SESSION_MESSAGE_PART_TYPE_TEXT,
                        content="edited connector reply",
                    )
                ],
            ),
            timeout=10,
        )

        delivered = wait_for_delivery_status(
            client,
            namespace,
            agent_name,
            session_id,
            pending.id,
            CONNECTOR_DELIVERY_DELIVERED,
        )
        assert message_text(delivered) == "edited connector reply"
        assert len(runtime.deliveries) == 1
        delivery = runtime.deliveries[0]
        assert delivery["text"] == "edited connector reply"
        assert delivery["connectorName"] == connector_name
        assert delivery["externalConversationId"] == "room-1"
    finally:
        runtime.stop()
