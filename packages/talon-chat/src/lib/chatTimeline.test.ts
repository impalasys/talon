import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { hasMessageCompaction } from "./chatTimeline.ts";

describe("hasMessageCompaction", () => {
  it("recognizes generated, numeric, and local compaction part representations", () => {
    for (const partType of [13, "SESSION_MESSAGE_PART_TYPE_COMPACTION", "compaction"]) {
      assert.equal(hasMessageCompaction({ parts: [{ partType }] }), true);
    }
  });

  it("does not treat ordinary message parts as a compaction marker", () => {
    assert.equal(
      hasMessageCompaction({ parts: [{ partType: "SESSION_MESSAGE_PART_TYPE_TEXT", content: "hello" }] }),
      false,
    );
  });
});
