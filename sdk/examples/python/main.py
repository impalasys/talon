import grpc
from talon_client import CreateNamespaceRequest, ListNamespacesRequest, TalonClient
from talon_server import start


def main() -> None:
    with start() as server:
        with grpc.insecure_channel(server.grpc_endpoint) as channel:
            client = TalonClient(channel)
            client.namespaces.Create(CreateNamespaceRequest(name="example-app"))
            response = client.namespaces.List(ListNamespacesRequest())
            print(
                f"Talon is running at {server.grpc_endpoint} "
                f"with {len(response.namespaces)} namespace(s)"
            )


if __name__ == "__main__":
    main()
