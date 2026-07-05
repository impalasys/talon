from __future__ import annotations

import time

import grpc
import pytest

from talon_client import (
    CreateNamespaceRequest,
    CreateResourceRequest,
    CreateSessionRequest,
    GetSessionRequest,
    SendMessageRequest,
    TalonClient,
)
from talon_client.resources import AgentSpec, Model, ResourceManifest, ResourceMeta, ResourceSpec

import conftest
from e2e.stack import SessionStreamBuffer, start_aws_local_stack


PART_TYPE_TEXT = 1


def message_text(message):
    return "".join(part.content for part in message.parts if part.part_type == PART_TYPE_TEXT)


def last_assistant_message(messages):
    assistants = [message for message in messages if message.role == 2]
    return assistants[-1] if assistants else None


def ensure_namespace(stub, name):
    try:
        stub.namespaces.Create(CreateNamespaceRequest(name=name, recursive=True))
    except grpc.RpcError as err:
        if err.code() != grpc.StatusCode.ALREADY_EXISTS:
            raise


def create_agent_resource(stub, ns, name, spec):
    return stub.resources.Create(
        CreateResourceRequest(
            ns=ns,
            manifest=ResourceManifest(
                api_version="talon.impalasys.com/v1",
                kind="Agent",
                metadata=ResourceMeta(name=name, namespace=ns),
                spec=ResourceSpec(agent=spec),
            ),
        )
    ).resource


@pytest.fixture(scope="session")
def aws_e2e_stack(mock_llm_server):
    stack = start_aws_local_stack()
    try:
        yield stack
    finally:
        stack.stop()


def test_aws_e2e_dynamodb_sqs_and_unix_worker_streaming(aws_e2e_stack):
    raw_channel, channel = conftest.authenticated_gateway_channel(
        aws_e2e_stack.grpc_port, aws_e2e_stack.api_key
    )
    try:
        stub = TalonClient(channel)
        namespace = "talon-aws-e2e"
        agent = "aws-agent"
        ensure_namespace(stub, namespace)
        create_agent_resource(
            stub,
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
                                temperature=0.7,
                            ),
                        }
                    ]
                },
                system_prompt="You are a concise arithmetic assistant.",
            ),
        )
        session_id = stub.sessions.Create(
            CreateSessionRequest(agent=agent, ns=namespace)
        ).session_id

        with SessionStreamBuffer(
            grpc_port=aws_e2e_stack.grpc_port,
            api_key=aws_e2e_stack.api_key,
            namespace=namespace,
            agent=agent,
            session_id=session_id,
        ) as stream:
            stub.sessions.SendMessage(
                SendMessageRequest(
                    agent=agent,
                    session_id=session_id,
                    ns=namespace,
                    message="What is the square root of 144?",
                )
            )
            deadline = time.time() + 45
            final_messages = []
            while time.time() < deadline:
                response = stub.sessions.Get(
                    GetSessionRequest(agent=agent, session_id=session_id, ns=namespace)
                )
                final_messages = response.messages
                assistant = last_assistant_message(final_messages)
                if response.state == "IDLE" and assistant is not None:
                    break
                time.sleep(1)
            else:
                raise AssertionError("AWS E2E session did not finish")

            assistant = last_assistant_message(final_messages)
            assert assistant is not None
            assert "12" in message_text(assistant)
            assert stream.saw_text(), "Unix worker stream did not deliver any text events"
            assert stream.error is None
    finally:
        channel.close()
        raw_channel.close()
