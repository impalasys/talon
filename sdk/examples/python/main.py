import grpc
from talon_client.proto.talon.v1 import namespaces_pb2, namespaces_pb2_grpc
from talon_server import start


def main() -> None:
    with start() as server:
        with grpc.insecure_channel(server.grpc_endpoint) as channel:
            namespaces = namespaces_pb2_grpc.NamespaceServiceStub(channel)
            namespaces.Create(namespaces_pb2.CreateNamespaceRequest(name="example-app"))
            response = namespaces.List(namespaces_pb2.ListNamespacesRequest())
            print(
                f"Talon is running at {server.grpc_endpoint} "
                f"with {len(response.namespaces)} namespace(s)"
            )


if __name__ == "__main__":
    main()
