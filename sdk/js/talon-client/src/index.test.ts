import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { createTalonClient, v1Resources } from "./index.js";

describe("@impalasys/talon-client", () => {
  it("exports generated talon.v1 types", () => {
    const request = new v1Resources.ListResourcesRequest({ ns: "default", kind: "Agent" });
    assert.equal(request.ns, "default");
    assert.equal(request.kind, "Agent");
  });

  it("creates a gRPC-Web Talon clientset", () => {
    const client = createTalonClient("http://localhost:50051");
    assert.equal(typeof client.namespaces.list, "function");
    assert.equal(typeof client.resources.list, "function");
    assert.equal(typeof client.sessions.submitTurn, "function");
    assert.equal(typeof client.channels.streamEvents, "function");
    assert.equal(typeof client.workflows.createRun, "function");
    assert.equal(typeof client.knowledge.search, "function");
    assert.equal(typeof client.auth.exchangeOidcToken, "function");
  });

  it("requires a baseUrl", () => {
    assert.throws(
      () => createTalonClient({ baseUrl: "  " }),
      /TalonClient requires a baseUrl/,
    );
  });
});
