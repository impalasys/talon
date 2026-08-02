import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { getMessageAssistantTimeline } from "./chatTimeline.ts";

describe("getMessageAssistantTimeline", () => {
  it("preserves every compaction part in its original position", () => {
    const timeline = getMessageAssistantTimeline({
      parts: [
        { partType: "SESSION_MESSAGE_PART_TYPE_COMPACTION" },
        { partType: "SESSION_MESSAGE_PART_TYPE_TEXT", content: "before" },
        { partType: 13 },
        { partType: "SESSION_MESSAGE_PART_TYPE_TEXT", content: "after" },
      ],
    });

    assert.deepEqual(timeline, [
      { type: "compaction" },
      { type: "text", text: "before" },
      { type: "compaction" },
      { type: "text", text: "after" },
    ]);
  });

  it("prefers part order when a persisted message also has a derived timeline", () => {
    const timeline = getMessageAssistantTimeline({
      timeline: [{ type: "text", text: "derived" }],
      parts: [
        { partType: "SESSION_MESSAGE_PART_TYPE_TEXT", content: "before" },
        { partType: "SESSION_MESSAGE_PART_TYPE_COMPACTION" },
        { partType: "SESSION_MESSAGE_PART_TYPE_TEXT", content: "after" },
      ],
    });

    assert.deepEqual(timeline, [
      { type: "text", text: "before" },
      { type: "compaction" },
      { type: "text", text: "after" },
    ]);
  });
});
