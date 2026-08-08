import { useCallback, useLayoutEffect, useRef } from "react";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import type { TalonClient } from "@impalasys/talon-client";
import type { CopilotMessage } from "../lib/chatTimeline";
import { streamSessionPartEvents, type StreamEventItem } from "../lib/uiStream";
import type { SessionHistoryPage } from "./history";
import type { SessionTarget } from "./types";

type SessionGenerationClient = Pick<TalonClient["sessions"], "streamParts" | "stopGeneration">;
type RefreshSession = (target: SessionTarget, signal?: AbortSignal) => Promise<SessionHistoryPage | null>;

type UseSessionGenerationOptions = {
  client: SessionGenerationClient | undefined;
  currentSession: SessionTarget | null;
  currentSessionRef: MutableRefObject<SessionTarget | null>;
  messagesRef: MutableRefObject<CopilotMessage[]>;
  serverState: string;
  isSessionLive: boolean;
  isStopping: boolean;
  submissionAbortControllerRef: MutableRefObject<AbortController | null>;
  setMessages: Dispatch<SetStateAction<CopilotMessage[]>>;
  setStreamEvents: Dispatch<SetStateAction<StreamEventItem[]>>;
  setError: (error: Error | null) => void;
  setIsLoading: (value: boolean) => void;
  setIsResuming: (value: boolean) => void;
  setIsStopping: (value: boolean) => void;
  setLoadingStartedAt: (value: string | number | null) => void;
  setLoadingNow: (value: number) => void;
  refreshRuntime: RefreshSession;
  refreshNewestSessionPage: RefreshSession;
};

function sameSession(left: SessionTarget | null, right: SessionTarget | null) {
  return left?.ns === right?.ns && left?.agent === right?.agent && left?.sessionId === right?.sessionId;
}

function normalizeEpochToMilliseconds(value: unknown) {
  let normalized: number | null = null;
  if (typeof value === "bigint") {
    const bigintValue = value < BigInt(0) ? -value : value;
    if (bigintValue > BigInt(Number.MAX_SAFE_INTEGER)) return null;
    normalized = Number(value);
  } else if (typeof value === "string") {
    const numericValue = Number(value);
    normalized = Number.isFinite(numericValue) ? numericValue : Date.parse(value);
  } else if (typeof value === "number") {
    normalized = value;
  }
  if (typeof normalized !== "number" || !Number.isFinite(normalized) || normalized <= 0) return null;
  if (normalized >= 1e15) return Math.trunc(normalized / 1000);
  if (normalized >= 1e12) return Math.trunc(normalized);
  if (normalized >= 1e9) return Math.trunc(normalized * 1000);
  return null;
}

function processingStartTime(messages: CopilotMessage[]) {
  const latestUserMessage = [...messages].reverse().find((message) => message.role === "user");
  return latestUserMessage ? normalizeEpochToMilliseconds(latestUserMessage.createdAt) : null;
}

/**
 * Owns the long-lived generation transport: reconnecting an in-flight stream
 * and requesting a stop. Submission stays separate because it also owns the
 * composer, optimistic message, and image-upload workflows.
 */
export function useSessionGeneration({
  client,
  currentSession,
  currentSessionRef,
  messagesRef,
  serverState,
  isSessionLive,
  isStopping,
  submissionAbortControllerRef,
  setMessages,
  setStreamEvents,
  setError,
  setIsLoading,
  setIsResuming,
  setIsStopping,
  setLoadingStartedAt,
  setLoadingNow,
  refreshRuntime,
  refreshNewestSessionPage,
}: UseSessionGenerationOptions) {
  const resumeAbortControllerRef = useRef<AbortController | null>(null);
  const stopAbortControllerRef = useRef<AbortController | null>(null);
  const isStoppingRef = useRef(false);
  const resumeStreamRef = useRef<(target: SessionTarget, signal?: AbortSignal) => Promise<void>>(async () => undefined);

  const cancelResume = useCallback(() => {
    resumeAbortControllerRef.current?.abort();
    resumeAbortControllerRef.current = null;
  }, []);

  const startResume = useCallback((target: SessionTarget, delayMs = 0, allowWhileStopping = false) => {
    if (isStoppingRef.current && !allowWhileStopping) return;
    const controller = new AbortController();
    cancelResume();
    resumeAbortControllerRef.current = controller;
    setIsResuming(true);
    setLoadingStartedAt(processingStartTime(messagesRef.current) ?? Date.now());
    setLoadingNow(Date.now());
    const run = () => {
      if (!controller.signal.aborted && sameSession(currentSessionRef.current, target) && (allowWhileStopping || !isStoppingRef.current)) {
        void resumeStreamRef.current(target, controller.signal);
      }
    };
    if (delayMs > 0) window.setTimeout(run, delayMs);
    else run();
  }, [cancelResume, currentSessionRef, messagesRef, setIsResuming, setLoadingNow, setLoadingStartedAt]);

  const resumeStream = useCallback(async (target: SessionTarget, signal?: AbortSignal) => {
    try {
      if (!client?.streamParts) {
        throw new Error("TalonSession requires a Talon clientset with sessions.streamParts().");
      }
      await streamSessionPartEvents({
        events: client.streamParts(target, { signal }),
        setMessages,
        setStreamEvents,
        signal,
      });
    } catch (err) {
      if (!signal?.aborted) setError(err instanceof Error ? err : new Error(String(err)));
    } finally {
      if (!signal?.aborted && sameSession(currentSessionRef.current, target)) {
        const refreshed = await refreshNewestSessionPage(target).catch(() => null);
        if (refreshed?.state === "PROCESSING" && !isStoppingRef.current) {
          startResume(target, 250);
        } else {
          setIsResuming(false);
          setLoadingStartedAt(null);
        }
      }
    }
  }, [client, currentSessionRef, refreshNewestSessionPage, setError, setIsResuming, setLoadingStartedAt, setMessages, setStreamEvents, startResume]);
  resumeStreamRef.current = resumeStream;

  const waitForSessionToStop = useCallback(async (target: SessionTarget, signal?: AbortSignal) => {
    for (let attempt = 0; attempt < 40; attempt += 1) {
      if (signal?.aborted || !sameSession(currentSessionRef.current, target)) return null;
      const refreshed = await refreshRuntime(target, signal);
      if (signal?.aborted || !sameSession(currentSessionRef.current, target)) return null;
      if (refreshed?.state !== "PROCESSING") return refreshed;
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    return null;
  }, [currentSessionRef, refreshRuntime]);

  const stopGeneration = useCallback(async (runtimeSignal?: AbortSignal) => {
    if (!currentSessionRef.current || !isSessionLive || isStopping) return;
    const session = currentSessionRef.current;
    const stopController = new AbortController();
    const abortFromRuntime = () => stopController.abort();
    if (runtimeSignal) {
      if (runtimeSignal.aborted) stopController.abort();
      else runtimeSignal.addEventListener("abort", abortFromRuntime, { once: true });
    }
    const removeRuntimeAbort = () => runtimeSignal?.removeEventListener("abort", abortFromRuntime);
    stopAbortControllerRef.current?.abort();
    stopAbortControllerRef.current = stopController;
    isStoppingRef.current = true;
    setIsStopping(true);
    submissionAbortControllerRef.current?.abort();
    submissionAbortControllerRef.current = null;
    cancelResume();
    setIsLoading(false);
    setIsResuming(false);
    setLoadingStartedAt(null);

    const resumeIfStillProcessing = async () => {
      const refreshed = await refreshNewestSessionPage(session, stopController.signal).catch(() => null);
      if (refreshed?.state === "PROCESSING") startResume(session, 0, true);
    };

    try {
      if (!client?.stopGeneration) {
        throw new Error("TalonSession requires a Talon clientset with sessions.stopGeneration().");
      }
      await client.stopGeneration(session, { signal: stopController.signal });
      const stopped = await waitForSessionToStop(session, stopController.signal);
      if (stopController.signal.aborted || !sameSession(currentSessionRef.current, session)) return;
      if (!stopped) {
        setError(new Error("Stop was requested, but the session is still generating."));
        await resumeIfStillProcessing();
        return;
      }
      await refreshNewestSessionPage(session, stopController.signal);
      setIsResuming(false);
      setLoadingStartedAt(null);
      setError(null);
    } catch (err) {
      if (stopController.signal.aborted || !sameSession(currentSessionRef.current, session)) return;
      setError(err instanceof Error ? err : new Error(String(err)));
      await resumeIfStillProcessing();
    } finally {
      if (stopAbortControllerRef.current === stopController) stopAbortControllerRef.current = null;
      removeRuntimeAbort();
      if (!stopController.signal.aborted && sameSession(currentSessionRef.current, session)) {
        isStoppingRef.current = false;
        setIsStopping(false);
      }
    }
  }, [cancelResume, client, currentSessionRef, isSessionLive, isStopping, refreshNewestSessionPage, setError, setIsLoading, setIsResuming, setIsStopping, setLoadingStartedAt, startResume, submissionAbortControllerRef, waitForSessionToStop]);

  const reset = useCallback(() => {
    cancelResume();
    stopAbortControllerRef.current?.abort();
    stopAbortControllerRef.current = null;
    isStoppingRef.current = false;
  }, [cancelResume]);

  const previousSessionRef = useRef<SessionTarget | null>(currentSession);
  useLayoutEffect(() => {
    const previousSession = previousSessionRef.current;
    previousSessionRef.current = currentSession;
    if (!previousSession || (currentSession && sameSession(previousSession, currentSession))) return;
    reset();
  }, [currentSession?.agent, currentSession?.ns, currentSession?.sessionId, reset]);

  useLayoutEffect(() => {
    if (!currentSession || serverState !== "PROCESSING" || isStoppingRef.current) return;
    if (resumeAbortControllerRef.current && !resumeAbortControllerRef.current.signal.aborted) return;
    startResume(currentSession);
    return cancelResume;
  }, [cancelResume, currentSession, serverState, startResume]);

  useLayoutEffect(() => () => reset(), [reset]);

  return { cancelResume, isStoppingRef, reset, startResume, stopGeneration };
}
