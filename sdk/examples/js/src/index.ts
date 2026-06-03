import { createPromiseClient } from "@connectrpc/connect";
import { createGrpcTransport } from "@connectrpc/connect-node";
import { gateway, gatewayConnect } from "@impalasys/talon-client";
import { start } from "@impalasys/talon-server";

const server = await start();
try {
  const transport = createGrpcTransport({
    baseUrl: `http://${server.grpcEndpoint}`,
    httpVersion: "2",
  });
  const client = createPromiseClient(gatewayConnect.GatewayService, transport);

  await client.createNamespace(new gateway.CreateNamespaceRequest({ name: "example-app" }));
  const response = await client.listNamespaces(new gateway.ListNamespacesRequest());

  console.log(`Talon is running at ${server.grpcEndpoint} with ${response.namespaces.length} namespace(s)`);
} finally {
  await server.stop();
}
