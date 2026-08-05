import { useCallback, useEffect, useReducer, useRef } from "react";
import type { Dispatch, SetStateAction } from "react";
import {
  emptySessionRuntimeState,
  sameSessionTarget,
  SessionOperationRegistry,
  sessionRuntimeReducer,
  type RuntimeCommandHandler,
  type RuntimeOperationKind,
  type SessionRuntimeState,
  type SubmitInput,
} from "./runtime";
import type { SessionHistoryPage } from "./history";
import { mergeNewestCanonicalPage } from "./history";
import type { SessionClient } from "./client";
import type { SessionTarget } from "./types";
import type { CopilotMessage } from "../lib/chatTimeline";

export type UseSessionRuntimeOptions = {
  target: SessionTarget | null;
  client: Pick<SessionClient, "listMessages">;
  pageSize: number;
  pollIntervalMs?: number;
  submit?: RuntimeCommandHandler<SubmitInput>;
  stop?: RuntimeCommandHandler;
};

export type SessionRuntimeController = {
  state: SessionRuntimeState;
  isLive: boolean;
  setMessages: Dispatch<SetStateAction<CopilotMessage[]>>;
  setPhase: (phase: SessionRuntimeState["phase"]) => void;
  setServerState: (state: SessionRuntimeState["serverState"]) => void;
  setError: (error: Error | null) => void;
  refresh: (target?: SessionTarget, signal?: AbortSignal) => Promise<SessionHistoryPage | null>;
  loadOlder: (target?: SessionTarget, signal?: AbortSignal) => Promise<SessionHistoryPage | null>;
  clear: () => void;
  cancelAllOperations: () => void;
  beginOperation: (kind: RuntimeOperationKind, target?: SessionTarget) => AbortSignal;
  activateTarget: (target: SessionTarget, options?: { hydrate?: boolean }) => void;
  submit: (input: SubmitInput) => Promise<void>;
  stop: () => Promise<void>;
};

function linkAbortSignal(source: AbortSignal | undefined, controller: AbortController): () => void {
  if (!source) return () => undefined;
  if (source.aborted) controller.abort();
  const abort = () => controller.abort();
  source.addEventListener("abort", abort, { once: true });
  return () => source.removeEventListener("abort", abort);
}

export function useSessionRuntime({
  target,
  client,
  pageSize,
  pollIntervalMs = 1000,
  submit: submitHandler,
  stop: stopHandler,
}: UseSessionRuntimeOptions): SessionRuntimeController {
  const [state, dispatch] = useReducer(sessionRuntimeReducer, emptySessionRuntimeState);
  const registryRef = useRef(new SessionOperationRegistry());
  const epochRef = useRef(0);
  const stateRef = useRef(state);
  const activeTargetRef = useRef<SessionTarget | null>(target);
  const inputTargetKey = target ? `${target.ns}\u0000${target.agent}\u0000${target.sessionId}` : null;
  const inputTargetKeyRef = useRef<string | null>(inputTargetKey);
  const clientRef = useRef(client);
  const pageSizeRef = useRef(pageSize);
  const submitHandlerRef = useRef(submitHandler);
  const stopHandlerRef = useRef(stopHandler);
  stateRef.current = state;
  clientRef.current = client;
  pageSizeRef.current = pageSize;
  submitHandlerRef.current = submitHandler;
  stopHandlerRef.current = stopHandler;

  const beginOperation = useCallback((kind: RuntimeOperationKind, operationTarget?: SessionTarget) => {
    const nextTarget = operationTarget ?? stateRef.current.target ?? target;
    if (!nextTarget) {
      const controller = new AbortController();
      controller.abort();
      return controller.signal;
    }
    return registryRef.current.begin(kind, nextTarget, epochRef.current).controller.signal;
  }, [target]);

  const hydrate = useCallback(async (nextTarget: SessionTarget, epoch: number, kind: "hydrate" | "poll" = "hydrate") => {
    const operation = registryRef.current.begin(kind, nextTarget, epoch);
    dispatch({ type: "operation-started", kind, epoch });
    try {
      const page = await clientRef.current.listMessages(nextTarget, { pageSize: pageSizeRef.current, signal: operation.controller.signal });
      const effectiveTarget = stateRef.current.target ?? activeTargetRef.current;
      if (!registryRef.current.isCurrent(operation) || epoch !== epochRef.current || !sameSessionTarget(effectiveTarget, nextTarget)) return null;
      dispatch({ type: kind === "hydrate" ? "hydrated" : "history-updated", target: nextTarget, page, messages: page.messages, epoch } as any);
      return page;
    } catch (error) {
      if (!operation.controller.signal.aborted && registryRef.current.isCurrent(operation)) {
        dispatch({ type: "error", error: error instanceof Error ? error : new Error(String(error)), epoch });
      }
      return null;
    } finally {
      registryRef.current.finish(operation);
    }
  }, []);

  const activateTarget = useCallback((nextTarget: SessionTarget, options: { hydrate?: boolean } = {}) => {
    activeTargetRef.current = nextTarget;
    const epoch = epochRef.current + 1;
    epochRef.current = epoch;
    registryRef.current.cancelAll();
    dispatch({ type: "target-changed", target: nextTarget, epoch });
    if (options.hydrate !== false) void hydrate(nextTarget, epoch);
  }, [hydrate]);

  useEffect(() => {
    if (inputTargetKeyRef.current === inputTargetKey && !target && activeTargetRef.current) return;
    if (inputTargetKeyRef.current === inputTargetKey && stateRef.current.target === target) return;
    inputTargetKeyRef.current = inputTargetKey;
    const epoch = epochRef.current + 1;
    epochRef.current = epoch;
    activeTargetRef.current = target;
    registryRef.current.cancelAll();
    dispatch({ type: "target-changed", target, epoch });
    if (target) void hydrate(target, epoch);
  }, [hydrate, inputTargetKey, target]);

  useEffect(() => {
    const pollingTarget = target ?? activeTargetRef.current;
    if (!pollingTarget || state.target == null || state.phase !== "idle") return;
    let disposed = false;
    const poll = async () => {
      if (disposed || stateRef.current.phase !== "idle" || stateRef.current.serverState === "PROCESSING") return;
      const page = await hydrate(pollingTarget, epochRef.current, "poll");
      if (disposed || !page || page.state !== "PROCESSING") return;
    };
    const intervalId = window.setInterval(() => void poll(), pollIntervalMs);
    return () => {
      disposed = true;
      window.clearInterval(intervalId);
      registryRef.current.cancel("poll");
    };
  }, [pollIntervalMs, state.phase, state.serverState, state.target, target, hydrate]);

  useEffect(() => () => {
    registryRef.current.cancelAll();
  }, []);

  const setMessages = useCallback<Dispatch<SetStateAction<CopilotMessage[]>>>((next) => {
    dispatch({ type: "messages-replaced", messages: next, epoch: epochRef.current });
  }, []);

  const refresh = useCallback(async (refreshTarget = stateRef.current.target ?? undefined, signal?: AbortSignal) => {
    if (!refreshTarget) return null;
    const operation = registryRef.current.begin("poll", refreshTarget, epochRef.current);
    const unlink = linkAbortSignal(signal, operation.controller);
    dispatch({ type: "operation-started", kind: "poll", epoch: epochRef.current });
    try {
      const page = await clientRef.current.listMessages(refreshTarget, { pageSize: pageSizeRef.current, signal: operation.controller.signal });
      const effectiveTarget = stateRef.current.target ?? activeTargetRef.current;
      if (!registryRef.current.isCurrent(operation) || !sameSessionTarget(effectiveTarget, refreshTarget)) return null;
      const merged = mergeNewestCanonicalPage(stateRef.current.messages, page.messages);
      dispatch({ type: "history-updated", target: refreshTarget, page, messages: merged, epoch: epochRef.current });
      return page;
    } catch (error) {
      if (!operation.controller.signal.aborted && registryRef.current.isCurrent(operation)) {
        dispatch({ type: "error", error: error instanceof Error ? error : new Error(String(error)), epoch: epochRef.current });
      }
      return null;
    } finally {
      unlink();
      registryRef.current.finish(operation);
    }
  }, []);

  const loadOlder = useCallback(async (olderTarget = stateRef.current.target ?? undefined, signal?: AbortSignal) => {
    if (!olderTarget || !stateRef.current.history.beforeMessageId) return null;
    const operation = registryRef.current.begin("paginate", olderTarget, epochRef.current);
    const unlink = linkAbortSignal(signal, operation.controller);
    const beforeMessageId = stateRef.current.history.beforeMessageId;
    try {
      const page = await clientRef.current.listMessages(olderTarget, { pageSize: pageSizeRef.current, beforeMessageId, signal: operation.controller.signal });
      if (!registryRef.current.isCurrent(operation) || !sameSessionTarget(stateRef.current.target, olderTarget)) return null;
      const existingIds = new Set(stateRef.current.messages.map((message) => message.id));
      const messages = [...page.messages.filter((message) => !existingIds.has(message.id)), ...stateRef.current.messages];
      dispatch({ type: "history-updated", target: olderTarget, page, messages, epoch: epochRef.current });
      return page;
    } finally {
      unlink();
      registryRef.current.finish(operation);
    }
  }, []);

  const setPhase = useCallback((phase: SessionRuntimeState["phase"]) => {
    dispatch({ type: "phase", phase, epoch: epochRef.current });
  }, []);
  const setServerState = useCallback((serverState: SessionRuntimeState["serverState"]) => {
    dispatch({ type: "server-state", serverState, epoch: epochRef.current });
  }, []);
  const setError = useCallback((error: Error | null) => {
    dispatch({ type: "error", error, epoch: epochRef.current });
  }, []);
  const clear = useCallback(() => {
    registryRef.current.cancelAll();
    dispatch({ type: "cleared", epoch: epochRef.current });
  }, []);
  const cancelAllOperations = useCallback(() => registryRef.current.cancelAll(), []);

  const runCommand = useCallback(async <Input,>(
    kind: "submit" | "stop",
    phase: "submitting" | "stopping",
    input: Input,
    handler: RuntimeCommandHandler<Input> | undefined,
  ) => {
    const operationTarget = stateRef.current.target ?? activeTargetRef.current;
    if (!operationTarget) throw new Error("Cannot run a session command without an active session.");
    if (!handler) throw new Error(`Session runtime does not have a ${kind} handler.`);
    const operation = registryRef.current.begin(kind, operationTarget, epochRef.current);
    dispatch({ type: "phase", phase, epoch: epochRef.current });
    try {
      await handler(input, {
        target: operationTarget,
        epoch: epochRef.current,
        signal: operation.controller.signal,
      });
    } finally {
      const wasCurrent = registryRef.current.isCurrent(operation);
      registryRef.current.finish(operation);
      if (!operation.controller.signal.aborted && wasCurrent) {
        dispatch({ type: "phase", phase: "idle", epoch: epochRef.current });
      }
    }
  }, []);
  const submit = useCallback((input: SubmitInput) => runCommand("submit", "submitting", input, submitHandlerRef.current), [runCommand]);
  const stop = useCallback(() => runCommand("stop", "stopping", undefined, stopHandlerRef.current), [runCommand]);

  return {
    state,
    isLive: state.phase === "submitting" || state.phase === "resuming" || state.phase === "stopping" || state.serverState === "PROCESSING",
    setMessages,
    setPhase,
    setServerState,
    setError,
    refresh,
    loadOlder,
    clear,
    cancelAllOperations,
    beginOperation,
    activateTarget,
    submit,
    stop,
  };
}
