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

  it("keeps mixed tool-result parts structured for the details renderer", () => {
    const timeline = getMessageAssistantTimeline({
      parts: [{
        partType: "SESSION_MESSAGE_PART_TYPE_TOOL_RESULT",
        name: "inspect",
        payloadJson: JSON.stringify({
          tool_call_id: "call-1",
          tool_output: {
            summary: "Tool result catalog",
            content_parts: [
              { type: "text", text: "small text" },
              { type: "object_ref", object_ref: { key: "cas/large", media_type: "text/plain" } },
              { type: "object_ref", object_ref: { key: "cas/image", media_type: "image/png" } },
            ],
          },
        }),
      }],
    });

    const result = timeline[0] && timeline[0].type === "tool" ? timeline[0].result : undefined;
    assert.deepEqual(result, {
      summary: "Tool result catalog",
      content_parts: [
        { type: "text", text: "small text" },
        { type: "object_ref", object_ref: { key: "cas/large", media_type: "text/plain" } },
        { type: "object_ref", object_ref: { key: "cas/image", media_type: "image/png" } },
      ],
    });
  });
});
