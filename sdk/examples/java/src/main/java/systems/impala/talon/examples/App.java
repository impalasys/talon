package systems.impala.talon.examples;

import io.grpc.ManagedChannel;
import io.grpc.ManagedChannelBuilder;
import systems.impala.talon.server.TalonServer;
import talon.gateway.Gateway;
import talon.gateway.GatewayServiceGrpc;

public final class App {
    private App() {}

    public static void main(String[] args) throws Exception {
        try (TalonServer server = TalonServer.start()) {
            ManagedChannel channel = ManagedChannelBuilder
                .forTarget(server.grpcEndpoint())
                .usePlaintext()
                .build();
            try {
                GatewayServiceGrpc.GatewayServiceBlockingStub client =
                    GatewayServiceGrpc.newBlockingStub(channel);

                client.createNamespace(Gateway.CreateNamespaceRequest.newBuilder()
                    .setName("example-app")
                    .build());

                Gateway.ListNamespacesResponse response = client.listNamespaces(
                    Gateway.ListNamespacesRequest.newBuilder().build());
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

