import { useCallback, useRef, useState } from "react";
import type { CopilotMessage } from "../lib/chatTimeline";
import type { StreamEventItem } from "../lib/uiStream";
import type { SessionRuntimeController } from "./useSessionRuntime";
import { createLocalMessageId } from "./useSessionActions";
import { useSessionImageAttachments } from "./useSessionImageAttachments";
import { useSessionPresentationState } from "./useSessionPresentationState";
import { useSessionTranscriptUi } from "./useSessionTranscriptUi";
import { useToolResultHydration } from "./useToolResultHydration";
import type { TalonSessionProps } from "./TalonSessionTypes";
import type { SessionTarget } from "./types";

type Options = Pick<TalonSessionProps, "acceptedImageTypes" | "agent" | "gatewayClient" | "maxImageAttachments" | "maxImageBytes" | "onImageUpload"> & {
  currentSession: SessionTarget | null;
  error: Error | null;
  history: { hasMoreOlder: boolean; beforeMessageId: string | null };
  isSessionLive: boolean;
  loadOlderRuntime: SessionRuntimeController["loadOlder"];
  messages: CopilotMessage[];
  setError: (error: Error | null) => void;
};

/** Owns composer, hydration, and transcript UI state, leaving transport to the parent controller. */
export function useTalonSessionConversation({
  acceptedImageTypes = ["image/png", "image/jpeg", "image/gif", "image/webp"],
  agent,
  currentSession,
  error,
  gatewayClient,
  history,
  isSessionLive,
  loadOlderRuntime,
  maxImageAttachments = 4,
  maxImageBytes = 20 * 1024 * 1024,
  messages,
  onImageUpload,
  setError,
}: Options) {
  const [input, setInput] = useState("");
  const [loadingStartedAt, setLoadingStartedAt] = useState<string | number | null>(null);
  const [streamEvents, setStreamEvents] = useState<StreamEventItem[]>([]);
  const abortControllerRef = useRef<AbortController | null>(null);
  const images = useSessionImageAttachments({ acceptedImageTypes, createId: createLocalMessageId, maxImageAttachments, maxImageBytes, onError: setError, onUpload: onImageUpload });
  const hydration = useToolResultHydration(
    gatewayClient.cas,
    currentSession ? `${currentSession.ns}\u0000${currentSession.agent}\u0000${currentSession.sessionId}` : null,
  );
  const loadOlderHistory = useCallback(async () => {
    if (!currentSession || !history.beforeMessageId) return false;
    return Boolean(await loadOlderRuntime(currentSession));
  }, [currentSession, history.beforeMessageId, loadOlderRuntime]);
  const transcript = useSessionTranscriptUi({
    messages,
    sessionKey: currentSession ? `${currentSession.ns}\u0000${currentSession.agent}\u0000${currentSession.sessionId}` : null,
    isLive: isSessionLive,
    error,
    streamEvents,
    hydrationState: hydration.state,
    canLoadOlder: Boolean(currentSession && history.hasMoreOlder && history.beforeMessageId),
    onLoadOlder: loadOlderHistory,
  });
  const presentation = useSessionPresentationState({ abortControllerRef, currentSession, input, isSessionLive, loadingStartedAt, messages });
  return { abortControllerRef, hydration, images, input, loadingStartedAt, presentation, setInput, setLoadingStartedAt, setStreamEvents, streamEvents, transcript };
}
