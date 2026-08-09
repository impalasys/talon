import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  chatMessageToWireMessage,
  chatPartToWirePart,
  messagePartsForSessionUpdate,
  parseToolResultPayload,
  protoSessionPartsFromChatParts,
  SESSION_MESSAGE_PART_TYPE,
  toolResultObjectRef,
  wireMessageToChatMessage,
} from "./protocol.ts";

describe("session protocol codecs", () => {
  it("accepts malformed payloads without throwing", () => {
    assert.deepEqual(parseToolResultPayload("not-json"), {});
    assert.deepEqual(parseToolResultPayload(JSON.stringify(["not an object"])), {});
    assert.deepEqual(parseToolResultPayload(undefined), {});
  });

  it("does not recurse when a tool result has no object reference payload", () => {
    assert.doesNotThrow(() => toolResultObjectRef({ type: "tool-result" }));
    assert.equal(toolResultObjectRef({ type: "tool-result" }), undefined);
    assert.equal(toolResultObjectRef({ payloadJson: "{}" }), undefined);
    assert.deepEqual(
      toolResultObjectRef({ payloadJson: JSON.stringify({ object: { key: "cas/result" } }) }),
      { key: "cas/result" },
    );
  });

  it("keeps image object refs while removing UI-only preview URLs from RPC parts", () => {
    const parts = protoSessionPartsFromChatParts([
      { type: "text", text: "inspect this" },
      {
        type: "image",
        previewUrl: "blob:local-preview",
        payloadJson: JSON.stringify({ filename: "photo.png" }),
        object: { key: "uploads/photo.png", mediaType: "image/png" },
      },
    ]);

    assert.equal(parts.length, 2);
    assert.equal("previewUrl" in parts[1], false);
    assert.deepEqual(parts[1].object, { key: "uploads/photo.png", mediaType: "image/png" });
  });

  it("round-trips chat message parts without exposing preview URLs", () => {
    const message = {
      id: "assistant-1",
      role: "assistant" as const,
      content: "done",
      parts: [{ type: "text", text: "done", previewUrl: "blob:ignored" }],
    };
    const wire = chatMessageToWireMessage(message);
    assert.equal("previewUrl" in (wire.parts as any[])[0], false);
    assert.deepEqual(messagePartsForSessionUpdate(message), wire.parts);

    const chat = wireMessageToChatMessage({
      id: "assistant-1",
      role: "ROLE_ASSISTANT",
      parts: [{ partType: "SESSION_MESSAGE_PART_TYPE_TEXT", content: "done" }],
    });
    assert.equal(chat.id, "assistant-1");
    assert.equal(chat.role, "assistant");
    assert.deepEqual(chat.parts, [{ partType: "SESSION_MESSAGE_PART_TYPE_TEXT", content: "done" }]);
  });

  it("converts scalar parts to explicit text wire parts", () => {
    assert.deepEqual(chatPartToWirePart("hello"), {
      partType: SESSION_MESSAGE_PART_TYPE.TEXT,
      content: "hello",
    });
  });
});
