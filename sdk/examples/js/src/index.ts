import { createTalonClient, v1 } from "@impalasys/talon-client";
import { start } from "@impalasys/talon-server";

const server = await start();
try {
  const client = createTalonClient({ baseUrl: `http://${server.grpcEndpoint}` });

  await client.namespaces.create(new v1.CreateNamespaceRequest({ name: "example-app" }));
  const response = await client.namespaces.list(new v1.ListNamespacesRequest());

  console.log(`Talon is running at ${server.grpcEndpoint} with ${response.namespaces.length} namespace(s)`);
} finally {
  await server.stop();
}
