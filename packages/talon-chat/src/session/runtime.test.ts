import test from "node:test";
import assert from "node:assert/strict";
import {
  emptySessionRuntimeState,
  SessionOperationRegistry,
  sessionRuntimeReducer,
  type SessionRuntimeState,
} from "./runtime.ts";

const target = { ns: "default", agent: "assistant", sessionId: "session-a" };
const page = {
  messages: [{ id: "m-1", role: "assistant", content: "hello" }],
  state: "PROCESSING",
  hasMore: false,
  nextBeforeMessageId: null,
  hasMoreOlder: false,
  beforeMessageId: null,
};

test("runtime reducer makes processing hydration live and ignores stale epochs", () => {
  const hydrating = sessionRuntimeReducer(emptySessionRuntimeState, {
    type: "target-changed",
    target,
    epoch: 2,
  });
  const hydrated = sessionRuntimeReducer(hydrating, {
    type: "hydrated",
    target,
    page,
    epoch: 2,
  });
  assert.equal(hydrated.phase, "resuming");
  assert.equal(hydrated.serverState, "PROCESSING");
  assert.equal(hydrated.messages[0]?.id, "m-1");

  const stale = sessionRuntimeReducer(hydrated, {
    type: "messages-replaced",
    messages: [{ id: "stale", role: "user", content: "wrong session" }],
    epoch: 1,
  });
  assert.equal(stale, hydrated);
});

test("operation registry cancels replacement and all session work", () => {
  const registry = new SessionOperationRegistry();
  const first = registry.begin("hydrate", target, 1);
  const replacement = registry.begin("hydrate", target, 1);
  assert.equal(first.controller.signal.aborted, true);
  assert.equal(registry.isCurrent(first), false);
  assert.equal(registry.isCurrent(replacement), true);

  registry.cancelAll();
  assert.equal(replacement.controller.signal.aborted, true);
  assert.equal(registry.isCurrent(replacement), false);
});

test("runtime state retains canonical history while replacing messages", () => {
  const state: SessionRuntimeState = {
    ...emptySessionRuntimeState,
    target,
    epoch: 1,
    messages: [{ id: "local-1", role: "user", content: "optimistic" }],
    history: {
      messages: [{ id: "local-1", role: "user", content: "optimistic" }],
      hasMoreOlder: true,
      beforeMessageId: "m-0",
    },
  };
  const next = sessionRuntimeReducer(state, {
    type: "messages-replaced",
    messages: [{ id: "m-1", role: "assistant", content: "canonical" }],
    epoch: 1,
  });
  assert.equal(next.history.hasMoreOlder, true);
  assert.deepEqual(next.history.messages, next.messages);
});
