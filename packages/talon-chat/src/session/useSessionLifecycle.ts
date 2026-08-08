import { useCallback, useEffect, useRef } from "react";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import type { TalonClient } from "@impalasys/talon-client";
import type { CopilotMessage } from "../lib/chatTimeline";
import type { StreamEventItem } from "../lib/uiStream";
import type { SessionTarget } from "./types";

type SessionLifecycleClient = Pick<TalonClient["sessions"], "clear">;

type UseSessionLifecycleOptions = {
  client: SessionLifecycleClient | undefined;
  currentSession: SessionTarget | null;
  currentSessionRef: MutableRefObject<SessionTarget | null>;
  requestedSessionKey: string;
  submissionAbortControllerRef: MutableRefObject<AbortController | null>;
  resourceAbortControllerRef: MutableRefObject<AbortController | null>;
  messagesRef: MutableRefObject<CopilotMessage[]>;
  emptyMessages: CopilotMessage[];
  clearRuntime: () => void;
  resetGeneration: () => void;
  resetTranscriptUi: () => void;
  invalidateToolResultHydration: () => void;
  resetResourcePane: () => void;
  setStreamEvents: Dispatch<SetStateAction<StreamEventItem[]>>;
  setError: (error: Error | null) => void;
  setIsLoading: (value: boolean) => void;
  setIsResuming: (value: boolean) => void;
  setIsStopping: (value: boolean) => void;
  setSessionState: (value: string | null) => void;
  setLoadingStartedAt: (value: string | number | null) => void;
};

function sameSession(left: SessionTarget | null, right: SessionTarget | null) {
  return left?.ns === right?.ns && left?.agent === right?.agent && left?.sessionId === right?.sessionId;
}

/**
 * Owns client-side cleanup at session boundaries. Transport generation itself
 * remains in useSessionGeneration; this hook only clears UI/runtime state when
 * a session changes or is explicitly cleared.
 */
export function useSessionLifecycle({
  client,
  currentSession,
  currentSessionRef,
  requestedSessionKey,
  submissionAbortControllerRef,
  resourceAbortControllerRef,
  messagesRef,
  emptyMessages,
  clearRuntime,
  resetGeneration,
  resetTranscriptUi,
  invalidateToolResultHydration,
  resetResourcePane,
  setStreamEvents,
  setError,
  setIsLoading,
  setIsResuming,
  setIsStopping,
  setSessionState,
  setLoadingStartedAt,
}: UseSessionLifecycleOptions) {
  const previousSessionRef = useRef<SessionTarget | null>(null);

  useEffect(() => {
    const previousSession = previousSessionRef.current;
    previousSessionRef.current = currentSession;
    if (!previousSession || (currentSession && sameSession(previousSession, currentSession))) return;

    submissionAbortControllerRef.current?.abort();
    submissionAbortControllerRef.current = null;
    setIsStopping(false);
    setIsLoading(false);
    setIsResuming(false);
    setLoadingStartedAt(null);
    setStreamEvents([]);
    resetTranscriptUi();
    invalidateToolResultHydration();
  }, [
    currentSession,
    invalidateToolResultHydration,
    resetTranscriptUi,
    setIsLoading,
    setIsResuming,
    setIsStopping,
    setLoadingStartedAt,
    setStreamEvents,
    submissionAbortControllerRef,
  ]);

  // A pane is scoped to the requested session as well as a session created in-place.
  useEffect(() => {
    resetResourcePane();
  }, [requestedSessionKey, resetResourcePane]);

  const clearLocalSession = useCallback(() => {
    submissionAbortControllerRef.current?.abort();
    submissionAbortControllerRef.current = null;
    resetGeneration();
    resourceAbortControllerRef.current?.abort();
    resourceAbortControllerRef.current = null;
    clearRuntime();
    messagesRef.current = emptyMessages;
    setStreamEvents([]);
    setError(null);
    setIsLoading(false);
    setIsResuming(false);
    setSessionState(null);
    setIsStopping(false);
    setLoadingStartedAt(null);
    resetTranscriptUi();
    invalidateToolResultHydration();
    resetResourcePane();
  }, [
    clearRuntime,
    emptyMessages,
    invalidateToolResultHydration,
    messagesRef,
    resetGeneration,
    resetResourcePane,
    resetTranscriptUi,
    resourceAbortControllerRef,
    setError,
    setIsLoading,
    setIsResuming,
    setIsStopping,
    setLoadingStartedAt,
    setSessionState,
    setStreamEvents,
    submissionAbortControllerRef,
  ]);

  const clearSession = useCallback(async () => {
    const session = currentSessionRef.current;
    if (session) {
      try {
        if (!client?.clear) {
          throw new Error("TalonSession requires a Talon clientset with sessions.clear().");
        }
        await client.clear(session);
      } catch (error) {
        setError(error instanceof Error ? error : new Error(String(error)));
      }
    }
    clearLocalSession();
  }, [clearLocalSession, client, currentSessionRef, setError]);

  return { clearSession };
}
