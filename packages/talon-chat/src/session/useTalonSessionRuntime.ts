import { useCallback, useMemo, useRef } from "react";
import type { GatewayClientLike } from "./TalonSessionTypes";
import { normalizeHistoryPage } from "./history";
import { useSessionRuntime } from "./useSessionRuntime";
import type { SessionTarget } from "./types";

type UseTalonSessionRuntimeOptions = {
  agent: string;
  gatewayClient: GatewayClientLike;
  historyPageSize: number;
  historyMessageLimit: number;
  namespace: string;
  sessionId?: string;
};

/** Adapts gateway history calls to the generic session runtime and exposes its phase helpers. */
export function useTalonSessionRuntime({
  agent,
  gatewayClient,
  historyPageSize,
  historyMessageLimit,
  namespace,
  sessionId,
}: UseTalonSessionRuntimeOptions) {
  const target = useMemo<SessionTarget | null>(
    () => sessionId ? { ns: namespace, agent, sessionId } : null,
    [agent, namespace, sessionId],
  );
  const client = useMemo(() => ({
    listMessages: async (session: SessionTarget, options?: { beforeMessageId?: string | null; pageSize?: number }) => {
      const response = await gatewayClient.sessions.listMessages({
        ...session,
        pageSize: options?.pageSize,
        beforeMessageId: options?.beforeMessageId || undefined,
      });
      return normalizeHistoryPage(response);
    },
  }), [gatewayClient]);
  const submitRef = useRef<((input: any, context: any) => Promise<void>) | null>(null);
  const stopRef = useRef<((context: any) => Promise<void>) | null>(null);
  const runtime = useSessionRuntime({
    target,
    client,
    pageSize: Math.max(1, Math.trunc(historyPageSize || historyMessageLimit || 50)),
    submit: (input, context) => submitRef.current?.(input, context) ?? Promise.resolve(),
    stop: (_input, context) => stopRef.current?.(context) ?? Promise.resolve(),
  });
  const { setPhase, setServerState } = runtime;
  const setIsLoading = useCallback((value: boolean) => setPhase(value ? "submitting" : "idle"), [setPhase]);
  const setIsResuming = useCallback((value: boolean) => setPhase(value ? "resuming" : "idle"), [setPhase]);
  const setIsStopping = useCallback((value: boolean) => setPhase(value ? "stopping" : "idle"), [setPhase]);
  const setSessionState = useCallback((value: string | null) => {
    setServerState(value === "PROCESSING" ? "PROCESSING" : value === "ERROR" ? "ERROR" : value ? "IDLE" : "UNKNOWN");
  }, [setServerState]);

  return { runtime, setIsLoading, setIsResuming, setIsStopping, setSessionState, submitRef, stopRef };
}
