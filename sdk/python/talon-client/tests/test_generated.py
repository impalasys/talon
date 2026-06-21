from talon_client import TalonClient, api_pb2, api_pb2_grpc


class FakeChannel:
    def unary_unary(self, path, **kwargs):
        return (path, kwargs)

    def unary_stream(self, path, **kwargs):
        return (path, kwargs)


def test_generated_talon_v1_types_are_available() -> None:
    request = api_pb2.ListResourcesRequest(ns="default", kind="Agent")
    assert request.ns == "default"
    assert request.kind == "Agent"


def test_talon_client_exposes_service_stubs() -> None:
    client = TalonClient(FakeChannel())

    assert isinstance(client.namespaces, api_pb2_grpc.NamespaceServiceStub)
    assert isinstance(client.resources, api_pb2_grpc.ResourceServiceStub)
    assert isinstance(client.sessions, api_pb2_grpc.SessionServiceStub)
    assert isinstance(client.channels, api_pb2_grpc.ChannelServiceStub)
    assert isinstance(client.workflows, api_pb2_grpc.WorkflowServiceStub)
    assert isinstance(client.knowledge, api_pb2_grpc.KnowledgeServiceStub)
    assert isinstance(client.auth, api_pb2_grpc.AuthServiceStub)
