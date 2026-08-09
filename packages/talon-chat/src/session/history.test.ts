import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  mergeNewestCanonicalPage,
  normalizeHistoryPage,
  validateCursorAdvances,
} from "./history.ts";

describe("session history", () => {
  it("requires canonical server message IDs", () => {
    assert.throws(
      () => normalizeHistoryPage({ messages: [{ role: "ROLE_ASSISTANT", content: "missing id" }] }),
      /canonical id/,
    );
  });

  it("normalizes snake_case pagination fields and preserves canonical IDs", () => {
    const page = normalizeHistoryPage({
      state: "IDLE",
      has_more: true,
      next_before_message_id: "older-than-this",
      messages: [{
        id: "server-message-1",
        role: "ROLE_USER",
        content: "hello",
        created_at: "1777755592000000",
      }],
    });
    assert.equal(page.messages[0]?.id, "server-message-1");
    assert.equal(page.hasMoreOlder, true);
    assert.equal(page.beforeMessageId, "older-than-this");
  });

  it("replaces updated canonical messages while retaining optimistic local messages", () => {
    const merged = mergeNewestCanonicalPage(
      [
        { id: "local-user-1", role: "user", content: "optimistic prompt" },
        { id: "assistant-1", role: "assistant", content: "partial" },
      ],
      [{ id: "assistant-1", role: "assistant", content: "canonical final" }],
      { preserveOptimistic: true },
    );
    assert.deepEqual(merged.map((message) => [message.id, message.content]), [
      ["local-user-1", "optimistic prompt"],
      ["assistant-1", "canonical final"],
    ]);
  });

  it("rejects a non-advancing pagination cursor", () => {
    assert.equal(validateCursorAdvances("cursor-1", "cursor-1"), false);
    assert.equal(validateCursorAdvances("cursor-1", "cursor-2"), true);
    assert.equal(validateCursorAdvances(null, "cursor-1"), true);
  });
});
