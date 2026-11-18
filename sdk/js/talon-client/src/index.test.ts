import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { createTalonClient, v1Resources } from "./index.js";
import * as llm from "./gen/proto/harness/llm_pb.js";

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
    assert.equal(typeof client.cas.getObject, "function");
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

describe("byte-range wire contract", () => {
  it("round-trips ChatContentPart byte ranges", () => {
    const part = new llm.ChatContentPart({
      content: { case: "text", value: "héllo wörld" },
      byteRange: new llm.ByteRange({ start: 0n, end: 5n }),
    });
    const decoded = llm.ChatContentPart.fromBinary(part.toBinary());
    assert.deepEqual(decoded, part);
    assert.equal(decoded.byteRange?.end, 5n);
  });

  it("round-trips ToolOutput byte ranges through JSON", () => {
    const output = new llm.ToolOutput({
      contentParts: [new llm.ChatContentPart({ content: { case: "text", value: "abc" } })],
      summary: "s",
      byteRange: new llm.ToolOutputByteRange({ start: 0n, end: 3n, nextByte: 3n }),
    });
    const decoded = llm.ToolOutput.fromJson(output.toJson());
    assert.deepEqual(decoded, output);
    assert.equal(decoded.byteRange?.nextByte, 3n);
  });
});
