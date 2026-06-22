package systems.impala.talon.examples;

import io.grpc.ManagedChannel;
import io.grpc.ManagedChannelBuilder;
import systems.impala.talon.server.TalonServer;
import talon.v1.NamespaceServiceGrpc;
import talon.v1.Namespaces;

public final class App {
    private App() {}

    public static void main(String[] args) throws Exception {
        try (TalonServer server = TalonServer.start()) {
            ManagedChannel channel = ManagedChannelBuilder
                .forTarget(server.grpcEndpoint())
                .usePlaintext()
                .build();
            try {
                NamespaceServiceGrpc.NamespaceServiceBlockingStub namespaces =
                    NamespaceServiceGrpc.newBlockingStub(channel);

                namespaces.create(Namespaces.CreateNamespaceRequest.newBuilder()
                    .setName("example-app")
                    .build());

                Namespaces.ListNamespacesResponse response = namespaces.list(
                    Namespaces.ListNamespacesRequest.newBuilder().build());
                System.out.printf(
                    "Talon is running at %s with %d namespace(s)%n",
                    server.grpcEndpoint(),
                    response.getNamespacesCount());
            } finally {
                channel.shutdownNow();
            }
        }
    }
}
