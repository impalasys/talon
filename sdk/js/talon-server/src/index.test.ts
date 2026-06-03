import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { TalonServer } from "./index.js";

describe("@impalasys/talon-server", () => {
  it("exports the server helper", () => {
    assert.equal(typeof TalonServer.start, "function");
  });
});
