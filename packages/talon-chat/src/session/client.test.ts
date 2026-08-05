import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { createSessionClient } from "./client.ts";

describe("session client adapter", () => {
  it("provides typed list/create/clear/stop calls with normalized arguments", async () => {
    let listRequest: any;
    const source = {
      listMessages: async (request: any) => {
        listRequest = request;
        return { items: [] };
      },
      create: async () => ({ sessionId: "created-session" }),
      clear: async () => undefined,
      stopGeneration: async (_target: any, options: any) => options,
    };
    const client = createSessionClient(source, (response) => ({
      ...response,
      messages: [],
      state: "IDLE",
      hasMore: false,
      nextBeforeMessageId: null,
      hasMoreOlder: false,
      beforeMessageId: null,
    }));
    const target = { ns: "ops", agent: "copilot", sessionId: "sess-1" };
    await client.listMessages(target, { beforeMessageId: "older", pageSize: 10 });
    assert.deepEqual(listRequest, {
      ns: "ops",
      agent: "copilot",
      sessionId: "sess-1",
      pageSize: 10,
      beforeMessageId: "older",
    });
    assert.deepEqual(await client.create({ ns: "ops", agent: "copilot" }), {
      ns: "ops",
      agent: "copilot",
      sessionId: "created-session",
    });
    await client.clear(target);
    const controller = new AbortController();
    await client.stopGeneration(target, controller.signal);
  });
});
