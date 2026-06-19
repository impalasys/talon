import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { gateway } from "./index.js";

describe("@impalasys/talon-client", () => {
  it("exports generated gateway types", () => {
    const request = new gateway.ListResourcesRequest({ ns: "default", kind: "Agent" });
    assert.equal(request.ns, "default");
    assert.equal(request.kind, "Agent");
  });
});
