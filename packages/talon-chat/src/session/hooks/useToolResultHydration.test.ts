import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { initialToolResultPartByteRange, toolResultPartText } from "./toolResultHydration.ts";

describe("toolResultPartText", () => {
  it("reads the generated Connect oneof text shape", () => {
    assert.equal(
      toolResultPartText({ content: { case: "text", value: "loaded result" } }),
      "loaded result",
    );
  });

  it("rejects non-text response variants", () => {
    assert.equal(toolResultPartText({ content: { case: "objectRef", value: {} } }), "");
  });

  it("uses the generated protobuf oneof shape for bounded reads", () => {
    assert.deepEqual(initialToolResultPartByteRange(), {
      start: 0n,
      limit: { case: "maxSize", value: 8192n },
    });
  });
});
