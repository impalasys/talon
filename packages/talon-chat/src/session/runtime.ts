// @ts-ignore The Node strip-types test runner requires explicit .ts resolution.
import type { CopilotMessage } from "../lib/chatTimeline.ts";
// @ts-ignore The Node strip-types test runner requires explicit .ts resolution.
import type { SessionHistoryPage, HistoryState } from "./history.ts";
import type { SessionTarget } from "./types";

export type SessionPhase =
  | "empty"
  | "hydrating"
  | "idle"
  | "submitting"
  | "resuming"
  | "stopping";

export type SessionServerState = "UNKNOWN" | "IDLE" | "PROCESSING" | "ERROR";

export type SessionRuntimeState = {
  target: SessionTarget | null;
  phase: SessionPhase;
  serverState: SessionServerState;
  messages: CopilotMessage[];
  history: HistoryState;
  error: Error | null;
  epoch: number;
};

export type RuntimeOperationKind =
  | "hydrate"
  | "paginate"
  | "submit"
  | "resume"
  | "stop"
  | "poll"
  | "resource";

export type RuntimeOperation = {
  kind: RuntimeOperationKind;
  target: SessionTarget;
  epoch: number;
  controller: AbortController;
};

export const emptyHistoryState: HistoryState = {
  messages: [],
  hasMoreOlder: false,
  beforeMessageId: null,
};

export const emptySessionRuntimeState: SessionRuntimeState = {
  target: null,
  phase: "empty",
  serverState: "UNKNOWN",
  messages: [],
  history: emptyHistoryState,
  error: null,
  epoch: 0,
};

export function sameSessionTarget(left: SessionTarget | null, right: SessionTarget | null): boolean {
  return Boolean(
    left && right
      && left.ns === right.ns
      && left.agent === right.agent
      && left.sessionId === right.sessionId,
  );
}

function stateForPage(page: SessionHistoryPage, target: SessionTarget, epoch: number): SessionRuntimeState {
  const serverState: SessionServerState = page.state === "PROCESSING"
    ? "PROCESSING"
    : page.state === "ERROR"
      ? "ERROR"
      : "IDLE";
  return {
    target,
    phase: serverState === "PROCESSING" ? "resuming" : "idle",
    serverState,
    messages: page.messages,
    history: {
      messages: page.messages,
      hasMoreOlder: page.hasMoreOlder,
      beforeMessageId: page.beforeMessageId,
    },
    error: null,
    epoch,
  };
}

export type SessionRuntimeAction =
  | { type: "target-changed"; target: SessionTarget | null; epoch: number }
  | { type: "operation-started"; kind: RuntimeOperationKind; epoch: number }
  | { type: "hydrated"; target: SessionTarget; page: SessionHistoryPage; epoch: number }
  | { type: "history-updated"; target: SessionTarget; page: SessionHistoryPage; messages: CopilotMessage[]; epoch: number }
  | { type: "messages-replaced"; messages: CopilotMessage[] | ((previous: CopilotMessage[]) => CopilotMessage[]); epoch?: number }
  | { type: "server-state"; serverState: SessionServerState; epoch?: number }
  | { type: "phase"; phase: SessionPhase; epoch?: number }
  | { type: "error"; error: Error | null; epoch?: number }
  | { type: "cleared"; epoch?: number };

function acceptsEpoch(state: SessionRuntimeState, epoch?: number): boolean {
  return epoch == null || epoch === state.epoch;
}

export function sessionRuntimeReducer(
  state: SessionRuntimeState,
  action: SessionRuntimeAction,
): SessionRuntimeState {
  switch (action.type) {
    case "target-changed":
      return action.target
        ? {
            ...emptySessionRuntimeState,
            target: action.target,
            phase: "hydrating",
            epoch: action.epoch,
          }
        : { ...emptySessionRuntimeState, epoch: action.epoch };
    case "operation-started":
      return acceptsEpoch(state, action.epoch) ? { ...state, error: null } : state;
    case "hydrated":
      return acceptsEpoch(state, action.epoch) && sameSessionTarget(state.target, action.target)
        ? stateForPage(action.page, action.target, action.epoch)
        : state;
    case "history-updated":
      return acceptsEpoch(state, action.epoch) && sameSessionTarget(state.target, action.target)
        ? {
            ...state,
            messages: action.messages,
            history: {
              messages: action.messages,
              hasMoreOlder: action.page.hasMoreOlder,
              beforeMessageId: action.page.beforeMessageId,
            },
            serverState: action.page.state === "PROCESSING"
              ? "PROCESSING"
              : action.page.state === "ERROR" ? "ERROR" : "IDLE",
            phase: action.page.state === "PROCESSING"
              ? "resuming"
              : state.phase === "submitting" || state.phase === "stopping" ? state.phase : "idle",
          }
        : state;
    case "messages-replaced":
      if (!acceptsEpoch(state, action.epoch)) return state;
      {
        const messages = typeof action.messages === "function" ? action.messages(state.messages) : action.messages;
        return { ...state, messages, history: { ...state.history, messages } };
      }
    case "server-state":
      return acceptsEpoch(state, action.epoch)
        ? {
            ...state,
            serverState: action.serverState,
            phase: action.serverState === "PROCESSING" ? "resuming" : state.phase,
          }
        : state;
    case "phase":
      return acceptsEpoch(state, action.epoch) ? { ...state, phase: action.phase } : state;
    case "error":
      return acceptsEpoch(state, action.epoch) ? { ...state, error: action.error, serverState: action.error ? "ERROR" : state.serverState } : state;
    case "cleared":
      return acceptsEpoch(state, action.epoch)
        ? { ...emptySessionRuntimeState, epoch: state.epoch, target: state.target }
        : state;
    default:
      return state;
  }
}

export class SessionOperationRegistry {
  private operations = new Map<RuntimeOperationKind, RuntimeOperation>();

  begin(kind: RuntimeOperationKind, target: SessionTarget, epoch: number): RuntimeOperation {
    this.operations.get(kind)?.controller.abort();
    const operation = { kind, target, epoch, controller: new AbortController() };
    this.operations.set(kind, operation);
    return operation;
  }

  isCurrent(operation: RuntimeOperation): boolean {
    return this.operations.get(operation.kind) === operation
      && !operation.controller.signal.aborted;
  }

  finish(operation: RuntimeOperation): void {
    if (this.operations.get(operation.kind) === operation) this.operations.delete(operation.kind);
  }

  cancel(kind: RuntimeOperationKind): void {
    const operation = this.operations.get(kind);
    operation?.controller.abort();
    this.operations.delete(kind);
  }

  cancelAll(): void {
    for (const operation of this.operations.values()) operation.controller.abort();
    this.operations.clear();
  }
}
