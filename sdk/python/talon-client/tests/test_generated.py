from talon_client import (
    TalonClient,
    auth_pb2_grpc,
    channels_pb2_grpc,
    knowledge_pb2_grpc,
    namespaces_pb2_grpc,
    resources_pb2,
    resources_pb2_grpc,
    sessions_pb2_grpc,
    workflows_pb2_grpc,
)


class FakeChannel:
    def unary_unary(self, path, **kwargs):
        return (path, kwargs)

    def unary_stream(self, path, **kwargs):
        return (path, kwargs)


def test_generated_talon_v1_types_are_available() -> None:
    request = resources_pb2.ListResourcesRequest(ns="default", kind="Agent")
    assert request.ns == "default"
    assert request.kind == "Agent"


def test_talon_client_exposes_service_stubs() -> None:
    client = TalonClient(FakeChannel())

    assert isinstance(client.namespaces, namespaces_pb2_grpc.NamespaceServiceStub)
    assert isinstance(client.resources, resources_pb2_grpc.ResourceServiceStub)
    assert isinstance(client.sessions, sessions_pb2_grpc.SessionServiceStub)
    assert isinstance(client.channels, channels_pb2_grpc.ChannelServiceStub)
    assert isinstance(client.workflows, workflows_pb2_grpc.WorkflowServiceStub)
    assert isinstance(client.knowledge, knowledge_pb2_grpc.KnowledgeServiceStub)
    assert isinstance(client.auth, auth_pb2_grpc.AuthServiceStub)
