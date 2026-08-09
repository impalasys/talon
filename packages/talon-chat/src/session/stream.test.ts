import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { streamSessionPartEvents } from "./stream.ts";

async function collect(events: AsyncIterable<any>, signal?: AbortSignal) {
  const result = [];
  for await (const event of streamSessionPartEvents(events, signal)) result.push(event);
  return result;
}

describe("session stream adapter", () => {
  it("classifies numeric and string event kinds while preserving ordering", async () => {
    const events = collect((async function* () {
      yield {
        kind: "SESSION_MESSAGE_PART_EVENT_KIND_DELTA",
        messageId: "assistant-1",
        part: { partType: "SESSION_MESSAGE_PART_TYPE_TOOL_CALL", id: "call-1", name: "search", payloadJson: JSON.stringify({ input: { q: "x" } }) },
      };
      yield {
        kind: "SESSION_MESSAGE_PART_EVENT_KIND_DELTA",
        messageId: "assistant-1",
        part: { partType: 4, id: "call-1", payloadJson: JSON.stringify({ output: "done", tool_call_id: "call-1" }) },
      };
      yield {
        kind: "SESSION_MESSAGE_PART_EVENT_KIND_DELTA",
        messageId: "assistant-1",
        part: { partType: "SESSION_MESSAGE_PART_TYPE_USAGE", payloadJson: JSON.stringify({ total_tokens: 9 }) },
      };
      yield { kind: "SESSION_MESSAGE_PART_EVENT_KIND_DONE", messageId: "assistant-1" };
    })());
    assert.deepEqual((await events).map((event) => event.type), [
      "assistant-part", "tool-started", "assistant-part", "tool-result", "assistant-part", "usage", "stream-completed",
    ]);
  });

  it("reports empty stream completion and preserves reasoning/image parts", async () => {
    assert.deepEqual(await collect((async function* () {})()), [{ type: "stream-completed" }]);
    const events = await collect((async function* () {
      yield { kind: 1, messageId: "assistant-2", part: { partType: 2, content: "thinking" } };
      yield { kind: 1, messageId: "assistant-2", part: { partType: "SESSION_MESSAGE_PART_TYPE_IMAGE", object: { key: "image" } } };
    })());
    assert.deepEqual(events.map((event) => event.type), ["assistant-part", "assistant-part", "stream-completed"]);
  });

  it("classifies stream failures and stops promptly on abort", async () => {
    const failure = await collect((async function* () {
      yield { kind: 3, part: { content: "gateway failed" } };
    })());
    assert.equal(failure[0]?.type, "stream-failed");
    assert.equal((failure[0] as any).error.message, "gateway failed");

    const controller = new AbortController();
    controller.abort();
    assert.deepEqual(await collect((async function* () {
      yield { kind: "SESSION_MESSAGE_PART_EVENT_KIND_DONE" };
    })(), controller.signal), []);
  });
});
