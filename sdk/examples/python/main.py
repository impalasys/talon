import grpc
from talon_client.proto import gateway_pb2, gateway_pb2_grpc
from talon_server import start


def main() -> None:
    with start() as server:
        with grpc.insecure_channel(server.grpc_endpoint) as channel:
            client = gateway_pb2_grpc.GatewayServiceStub(channel)
            client.CreateNamespace(gateway_pb2.CreateNamespaceRequest(name="example-app"))
            response = client.ListNamespaces(gateway_pb2.ListNamespacesRequest())
            print(
                f"Talon is running at {server.grpc_endpoint} "
                f"with {len(response.namespaces)} namespace(s)"
            )


if __name__ == "__main__":
    main()

