import uuid

import grpc
import pytest

from e2e.blackbox import create_agent_resource, create_resource, ensure_namespace
from e2e.stack import E2EStack
from talon_client import (
    CreateSessionRequest,
    GetResourceRequest,
    MintAccessTokenRequest,
    TalonClient,
)
from talon_client.resources import (
    AgentSpec,
    Model,
    ResourceSpec,
    UsageLimit,
    UsagePolicySpec,
    UsageSelector,
)


def bearer_metadata(token: str) -> tuple[tuple[str, str], ...]:
    return (("authorization", f"Bearer {token}"),)


def test_subject_scoped_agent_session_policy_partitions_delegated_tokens(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    run_id = uuid.uuid4().hex[:8]
    namespace = f"talon-usage-{stack.name}-{run_id}"
    agent = f"usage-agent-{run_id}"

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
                        "model": Model(provider="mock", name="minimax"),
                    }
                ]
            },
            system_prompt="Usage policy test agent.",
        ),
    )
    create_resource(
        client,
        namespace,
        "UsagePolicy",
        "per-subject-session-creates",
        ResourceSpec(
            usage_policy=UsagePolicySpec(
                namespace_scope="self",
                hard=[
                    UsageLimit(
                        selector=UsageSelector(agent=agent),
                        metric="agent.sessions",
                        max=1,
                        window="1h",
                        subject_scope="identity",
                    )
                ],
            )
        ),
    )

    first_token = client.auth.MintAccessToken(
        MintAccessTokenRequest(
            namespace=namespace,
            agent=agent,
            expires_in=300,
            sub="browser-a",
        )
    ).access_token
    second_token = client.auth.MintAccessToken(
        MintAccessTokenRequest(
            namespace=namespace,
            agent=agent,
            expires_in=300,
            sub="browser-b",
        )
    ).access_token

    first = client.sessions.Create(
        CreateSessionRequest(ns=namespace, agent=agent),
        metadata=bearer_metadata(first_token),
    )
    assert first.session_id

    with pytest.raises(grpc.RpcError) as error:
        client.sessions.Create(
            CreateSessionRequest(ns=namespace, agent=agent),
            metadata=bearer_metadata(first_token),
        )
    assert error.value.code() == grpc.StatusCode.RESOURCE_EXHAUSTED

    second = client.sessions.Create(
        CreateSessionRequest(ns=namespace, agent=agent),
        metadata=bearer_metadata(second_token),
    )
    assert second.session_id

    policy = client.resources.Get(
        GetResourceRequest(
            ns=namespace,
            kind="UsagePolicy",
            name="per-subject-session-creates",
        )
    ).resource
    limit = policy.status.usage_policy.hard[0]
    assert limit.metric == "agent.sessions"
    assert limit.subject_scope == "identity"
    assert limit.used == 1
    assert limit.remaining == 0
    assert limit.exceeded is True
