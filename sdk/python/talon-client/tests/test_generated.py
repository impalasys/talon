from talon_client.proto import gateway_pb2


def test_generated_gateway_types_are_available() -> None:
    request = gateway_pb2.ListResourcesRequest(ns="default", kind="Agent")
    assert request.ns == "default"
    assert request.kind == "Agent"
