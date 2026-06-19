from pathlib import Path


def test_e2e_helpers_do_not_use_direct_gateway_rpc_or_v1_agent_crud():
    root = Path(__file__).resolve().parent / "e2e"
    forbidden = [
        "GatewayServiceStub",
        "proto.gateway_pb2",
        "proto.gateway_pb2_grpc",
        "/v1/ns/{namespace}/agents",
        "/v1/ns/{ns}/agents",
    ]
    offenders = []
    for path in root.rglob("*.py"):
        text = path.read_text()
        for pattern in forbidden:
            if pattern in text:
                offenders.append(f"{path.relative_to(root.parent)} contains {pattern!r}")
    assert not offenders, "\n".join(offenders)

