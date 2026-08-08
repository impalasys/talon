import { useCallback, useRef, useState } from "react";
import type { CopilotMessage } from "../lib/chatTimeline";
import type { StreamEventItem } from "../lib/uiStream";
import type { SessionRuntimeController } from "./hooks/useSessionRuntime";
import { createLocalMessageId } from "./useSessionActions";
import { useSessionAttachments } from "./hooks/useSessionAttachments";
import { useSessionPresentationState } from "./useSessionPresentationState";
import { useToolResultHydration } from "./hooks/useToolResultHydration";
import { useTranscriptExpansionState } from "./hooks/useTranscriptExpansionState";
import { useTranscriptPaginationAnchor } from "./hooks/useTranscriptPaginationAnchor";
import { useTranscriptScrollState } from "./hooks/useTranscriptScrollState";
import type { TalonSessionProps } from "./TalonSessionTypes";
import type { SessionTarget } from "./types";

type Options = Pick<TalonSessionProps, "acceptedAttachmentTypes" | "acceptedImageTypes" | "agent" | "gatewayClient" | "maxAttachments" | "maxAttachmentBytes" | "maxImageAttachments" | "maxImageBytes" | "onAttachmentUpload" | "onImageUpload"> & {
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
  acceptedAttachmentTypes,
  acceptedImageTypes,
  agent,
  currentSession,
  error,
  gatewayClient,
  history,
  isSessionLive,
  loadOlderRuntime,
  maxAttachments,
  maxAttachmentBytes,
  maxImageAttachments,
  maxImageBytes,
  messages,
  onAttachmentUpload,
  onImageUpload,
  setError,
}: Options) {
  const [input, setInput] = useState("");
  const [loadingStartedAt, setLoadingStartedAt] = useState<string | number | null>(null);
  const [streamEvents, setStreamEvents] = useState<StreamEventItem[]>([]);
  const abortControllerRef = useRef<AbortController | null>(null);
  const images = useSessionAttachments({
    acceptedTypes: acceptedAttachmentTypes ?? acceptedImageTypes ?? ["image/png", "image/jpeg", "image/gif", "image/webp"],
    createId: createLocalMessageId,
    maxAttachments: maxAttachments ?? maxImageAttachments ?? 4,
    maxBytes: maxAttachmentBytes ?? maxImageBytes ?? 20 * 1024 * 1024,
    onError: setError,
    onUpload: onAttachmentUpload ?? onImageUpload,
  });
  const hydration = useToolResultHydration(
    gatewayClient.cas,
    currentSession ? `${currentSession.ns}\u0000${currentSession.agent}\u0000${currentSession.sessionId}` : null,
  );
  const loadOlderHistory = useCallback(async () => {
    if (!currentSession || !history.beforeMessageId) return false;
    return Boolean(await loadOlderRuntime(currentSession));
  }, [currentSession, history.beforeMessageId, loadOlderRuntime]);
  const transcriptExpansion = useTranscriptExpansionState();
  const transcriptScroll = useTranscriptScrollState({
    messages,
    sessionKey: currentSession ? `${currentSession.ns}\u0000${currentSession.agent}\u0000${currentSession.sessionId}` : null,
    isLive: isSessionLive,
    error,
    streamEvents,
    hydrationState: hydration.state,
    expandedThinkingMessages: transcriptExpansion.expandedThinkingMessages,
    expandedToolItems: transcriptExpansion.expandedToolItems,
  });
  const transcriptPagination = useTranscriptPaginationAnchor({
    messages,
    transcriptRef: transcriptScroll.transcriptRef,
    canLoadOlder: Boolean(currentSession && history.hasMoreOlder && history.beforeMessageId),
    onLoadOlder: loadOlderHistory,
    onPrependCancelled: transcriptScroll.allowNextAutoScroll,
    onPrependStart: transcriptScroll.skipNextAutoScroll,
    onRestored: transcriptScroll.updateScrollThumb,
  });
  const handleTranscriptScroll = useCallback(() => {
    transcriptScroll.handleScroll();
    transcriptPagination.handleScroll();
  }, [transcriptPagination, transcriptScroll]);
  const resetTranscript = useCallback(() => {
    transcriptExpansion.reset();
    transcriptPagination.reset();
    transcriptScroll.reset();
  }, [transcriptExpansion, transcriptPagination, transcriptScroll]);
  const transcript = {
    ...transcriptExpansion,
    bottomRef: transcriptScroll.bottomRef,
    handleScroll: handleTranscriptScroll,
    markAutoScrollPinned: transcriptScroll.markAutoScrollPinned,
    reset: resetTranscript,
    scrollThumb: transcriptScroll.scrollThumb,
    transcriptRef: transcriptScroll.transcriptRef,
  };
  const presentation = useSessionPresentationState({ abortControllerRef, currentSession, input, isSessionLive, loadingStartedAt, messages });
  return { abortControllerRef, hydration, images, input, loadingStartedAt, presentation, setInput, setLoadingStartedAt, setStreamEvents, streamEvents, transcript };
}
