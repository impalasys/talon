"use client";

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type TalonClient } from "@impalasys/talon-client";
import { Activity } from "lucide-react";
import {
  getMessageContent,
  hydrateMessagesWithSteps,
  normalizeMessageRole,
  type CopilotMessage,
} from "./lib/chatTimeline";
import { TalonChatComposer, type TalonChatComposerVariant } from "./lib/TalonChatComposer";
import { type TalonBuiltInCommandName, type TalonChatCommand } from "./lib/commands";
import { ResourcePane } from "./lib/ResourcePane";
import {
  parseResourceUri,
  type ResourceViewModel,
} from "./lib/resourceUris";
import { type StreamEventItem } from "./lib/uiStream";
import { messagePartsForSessionUpdate } from "./session/protocol";
import { objectRefSizeBytes } from "./session/objectRefs";
import type { TalonChatObjectRef, TalonSessionHandle } from "./session/types";
import { useSessionRuntime } from "./session/hooks/useSessionRuntime";
import type { SessionTarget } from "./session/types";
import { SessionTranscript } from "./session/SessionTranscript";
import { SessionComposerDock } from "./session/SessionComposerDock";
import { useSessionImageAttachments } from "./session/useSessionImageAttachments";
import { useToolResultHydration } from "./session/hooks/useToolResultHydration";
import { useResourcePane } from "./session/hooks/useResourcePane";
import { useTranscriptExpansionState } from "./session/hooks/useTranscriptExpansionState";
import { useTranscriptPaginationAnchor } from "./session/hooks/useTranscriptPaginationAnchor";
import { useTranscriptScrollState } from "./session/hooks/useTranscriptScrollState";
import { fetchResourceFromGateway } from "./lib/resourceLoader";
import { useSessionGeneration } from "./session/useSessionGeneration";
import { createLocalMessageId, useSessionActions } from "./session/useSessionActions";
import { editableMessageContent, messageWithEditedContent, replaceMessageTextPart } from "./session/messageEditing";
import { copyMessageContent } from "./session/copyMessageContent";
import { formatWorkingDuration } from "./session/sessionTiming";
import { SessionStyles } from "./session/SessionStyles";
import { SessionMessage } from "./session/SessionMessage";
import {
  canCompareCanonicalMessageIds,
  isLocalMessageId,
  mergeNewestCanonicalPage,
  normalizeHistoryPage,
  normalizeMessageLabels,
  normalizeRawSessionMessage,
  type SessionHistoryPage,
} from "./session/history";

export type SessionServiceClientLike = {
  sessions: Pick<
    TalonClient["sessions"],
    "create" | "clear" | "listMessages" | "submitTurn" | "streamParts" | "stopGeneration"
  > & Partial<Pick<TalonClient["sessions"], "appendMessage" | "updateMessage">>;
}["sessions"];

export type CasServiceClientLike = Pick<TalonClient["cas"], "getObject">;

export type ArtifactServiceClientLike = Pick<
  TalonClient["artifacts"],
  "readArtifact" | "getArtifactMetadata"
>;

export type FileServiceClientLike = Pick<
  TalonClient["files"],
  "readFile" | "getFileMetadata"
>;

export type GatewayClientLike = {
  sessions: SessionServiceClientLike;
  cas?: CasServiceClientLike;
  artifacts?: ArtifactServiceClientLike;
  files?: FileServiceClientLike;
};

export type { ResourceViewModel };

export type TalonSessionCommandTarget = {
  type: "session";
  namespace: string;
  agent: string;
  sessionId: string | null;
};

export type TalonSessionCommand = TalonChatCommand<TalonSessionCommandTarget, CopilotMessage>;

export type { TalonChatObjectRef } from "./session/types";

export type TalonImageUploadContext = {
  file: File;
  namespace: string;
  agent: string;
  sessionId: string;
  signal: AbortSignal;
};

export type TalonImageUploadResult = TalonChatObjectRef | {
  object: TalonChatObjectRef;
  url?: string;
};

export type TalonSessionPendingImageAttachment = {
  id: string;
  file: File;
  previewUrl: string;
  object?: TalonChatObjectRef;
  status: "queued" | "uploading" | "ready" | "error";
  error?: string;
};

export type TalonSessionSubmitContext = {
  text: string;
  namespace: string;
  agent: string;
  sessionId: string | null;
  imageAttachments: ReadonlyArray<TalonSessionPendingImageAttachment>;
  ensureSession: () => Promise<TalonSessionHandle>;
  clearInput: () => void;
  refreshSession: () => Promise<void>;
};

export type TalonSessionMessageEditContext = {
  message: CopilotMessage;
  nextContent: string;
  namespace: string;
  agent: string;
  sessionId: string | null;
};

export type TalonSessionProps = {
  namespace: string;
  agent: string;
  gatewayClient: GatewayClientLike;
  sessionId?: string;
  onSessionChange?: (sessionId: string) => void;
  className?: string;
  style?: React.CSSProperties;
  placeholder?: string;
  autoFocus?: boolean;
  disabled?: boolean;
  historyPageSize?: number;
  historyMessageLimit?: number;
  historyStepLimit?: number;
  commands?: TalonSessionCommand[];
  enabledBuiltInCommands?: TalonBuiltInCommandName[];
  /**
   * Uploads an image selected in the composer and returns the stored object ref.
   * TalonSession performs client-side type and size checks for UX only; callers
   * must validate file type, size, and content again in this upload handler
   * before storing or processing the file.
   */
  onImageUpload?: (context: TalonImageUploadContext) => Promise<TalonImageUploadResult>;
  objectUrlForRef?: (object: TalonChatObjectRef) => string | undefined;
  maxImageAttachments?: number;
  /**
   * Client-side image size limit in bytes. This improves UX only and must be
   * enforced again by the onImageUpload implementation.
   */
  maxImageBytes?: number;
  /**
   * Client-side accepted image MIME types. This can be bypassed by callers and
   * must be enforced again by the onImageUpload implementation.
   */
  acceptedImageTypes?: string[];
  composerVariant?: TalonChatComposerVariant;
  composerStartAdornment?: React.ReactNode;
  composerEndAdornment?: React.ReactNode;
  onSubmitMessage?: (context: TalonSessionSubmitContext) => Promise<boolean | void> | boolean | void;
  allowMessageEditing?: boolean;
  onMessageEdit?: (context: TalonSessionMessageEditContext) => Promise<boolean | void> | boolean | void;
  enableDebugMessageEditing?: boolean;
  /**
   * Called when an artifact:// or file:// link is clicked.
   * If omitted, the built-in split pane opens when the matching client is available.
   */
  onResourceClick?: (uri: string) => void;
  /**
   * Override content fetch for the built-in resource pane (both kinds).
   */
  fetchResource?: (uri: string, signal: AbortSignal) => Promise<ResourceViewModel>;
};

export type TalonCopilotProps = TalonSessionProps;

const emptyMessages: CopilotMessage[] = [];
const DEFAULT_HISTORY_PAGE_SIZE = 50;
const DEFAULT_HISTORY_MESSAGE_LIMIT = 100;
const DEFAULT_HISTORY_STEP_LIMIT = 1000;
const LABEL_CONNECTOR_DELIVERY_STATUS = "talon.impalasys.com/connector-delivery-status";
const LABEL_CONNECTOR_DELIVERY_ERROR = "talon.impalasys.com/connector-delivery-error";

const talonChatFontFamily =
  'var(--talon-chat-font-family, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif)';

function isSameSession(
  left: { ns: string; agent: string; sessionId: string } | null,
  right: { ns: string; agent: string; sessionId: string } | null,
) {
  return (
    left?.ns === right?.ns &&
    left?.agent === right?.agent &&
    left?.sessionId === right?.sessionId
  );
}

export function TalonSession({
  namespace,
  agent,
  gatewayClient,
  sessionId,
  onSessionChange,
  className,
  style,
  placeholder = "Ask Talon to perform a task...",
  autoFocus = true,
  disabled = false,
  historyPageSize = DEFAULT_HISTORY_PAGE_SIZE,
  historyMessageLimit = DEFAULT_HISTORY_MESSAGE_LIMIT,
  historyStepLimit = DEFAULT_HISTORY_STEP_LIMIT,
  commands,
  enabledBuiltInCommands,
  onImageUpload,
  objectUrlForRef,
  maxImageAttachments = 4,
  maxImageBytes = 20 * 1024 * 1024,
  acceptedImageTypes = ["image/png", "image/jpeg", "image/gif", "image/webp"],
  composerVariant = "panel",
  composerStartAdornment,
  composerEndAdornment,
  onSubmitMessage,
  allowMessageEditing = false,
  onMessageEdit,
  enableDebugMessageEditing = false,
  onResourceClick: onResourceClickProp,
  fetchResource,
}: TalonSessionProps) {
  const initialSessionTarget = useMemo<SessionTarget | null>(
    () => sessionId ? { ns: namespace, agent, sessionId } : null,
    [agent, namespace, sessionId],
  );
  const runtimeClient = useMemo(() => ({
    listMessages: async (target: SessionTarget, options?: { beforeMessageId?: string | null; pageSize?: number; signal?: AbortSignal }) => {
      const response = await gatewayClient.sessions.listMessages({
        ...target,
        pageSize: options?.pageSize,
        beforeMessageId: options?.beforeMessageId || undefined,
      });
      return normalizeHistoryPage(response);
    },
  }), [gatewayClient]);
  const runtimeSubmitRef = useRef<((input: any, context: any) => Promise<void>) | null>(null);
  const runtimeStopRef = useRef<((context: any) => Promise<void>) | null>(null);
  const sessionRuntime = useSessionRuntime({
    target: initialSessionTarget,
    client: runtimeClient,
    pageSize: Math.max(1, Math.trunc(historyPageSize || historyMessageLimit || DEFAULT_HISTORY_PAGE_SIZE)),
    submit: (input, context) => runtimeSubmitRef.current?.(input, context) ?? Promise.resolve(),
    stop: (_input, context) => runtimeStopRef.current?.(context) ?? Promise.resolve(),
  });
  const {
    state: sessionRuntimeState,
    isLive: runtimeIsLive,
    setMessages,
    setPhase,
    setServerState,
    setError,
    refresh: refreshRuntime,
    loadOlder: loadOlderRuntime,
    clear: clearRuntime,
    activateTarget,
  } = sessionRuntime;
  const messages = sessionRuntimeState.messages;
  const currentSession = sessionRuntimeState.target;
  const isLoading = sessionRuntimeState.phase === "submitting";
  const isResuming = sessionRuntimeState.phase === "resuming";
  const isStopping = sessionRuntimeState.phase === "stopping";
  const sessionState = sessionRuntimeState.serverState === "UNKNOWN" ? null : sessionRuntimeState.serverState;
  const isSessionLive = runtimeIsLive;
  const setIsLoading = useCallback((value: boolean) => setPhase(value ? "submitting" : "idle"), [setPhase]);
  const setIsResuming = useCallback((value: boolean) => setPhase(value ? "resuming" : "idle"), [setPhase]);
  const setIsStopping = useCallback((value: boolean) => setPhase(value ? "stopping" : "idle"), [setPhase]);
  const setSessionState = useCallback((value: string | null) => {
    setServerState(value === "PROCESSING" ? "PROCESSING" : value === "ERROR" ? "ERROR" : value ? "IDLE" : "UNKNOWN");
  }, [setServerState]);
  const [input, setInput] = useState("");
  const {
    addFiles: addImageFiles,
    attachments: imageAttachments,
    attachmentsRef: imageAttachmentsRef,
    remove: removeImageAttachment,
    replace: setImageAttachments,
    uploadQueued: uploadQueuedImages,
  } = useSessionImageAttachments({
    acceptedImageTypes,
    createId: createLocalMessageId,
    maxImageAttachments,
    maxImageBytes,
    onError: setError,
    onUpload: onImageUpload,
  });
  const [loadingStartedAt, setLoadingStartedAt] = useState<string | number | null>(null);
  const [loadingNow, setLoadingNow] = useState(Date.now());
  const error = sessionRuntimeState.error;
  const [streamEvents, setStreamEvents] = useState<StreamEventItem[]>([]);
  const {
    state: toolResultHydration,
    resultFor: toolResultFor,
    hydrate: hydrateToolResultForExpandedItem,
    invalidate: invalidateToolResultHydration,
  } = useToolResultHydration(
    gatewayClient?.cas,
    currentSession ? `${currentSession.ns}\u0000${currentSession.agent}\u0000${currentSession.sessionId}` : null,
  );
  const missingResourceClientWarnedRef = useRef(false);
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [editingMessageValue, setEditingMessageValue] = useState("");
  const [reviewActionMessageId, setReviewActionMessageId] = useState<string | null>(null);
  const hasMoreHistory = sessionRuntimeState.history.hasMoreOlder;
  const nextBeforeMessageId = sessionRuntimeState.history.beforeMessageId;
  const loadOlderHistory = useCallback(async () => {
    if (!currentSession || !nextBeforeMessageId) return false;
    return Boolean(await loadOlderRuntime(currentSession));
  }, [currentSession, loadOlderRuntime, nextBeforeMessageId]);
  const transcriptExpansion = useTranscriptExpansionState();
  const {
    allowNextAutoScroll,
    bottomRef,
    handleScroll: handleTranscriptScrollState,
    markAutoScrollPinned,
    reset: resetTranscriptScroll,
    scrollThumb,
    skipNextAutoScroll,
    transcriptRef: scrollContainerRef,
    updateScrollThumb,
  } = useTranscriptScrollState({
    messages,
    sessionKey: currentSession ? `${currentSession.ns}\u0000${currentSession.agent}\u0000${currentSession.sessionId}` : null,
    isLive: isSessionLive,
    error,
    streamEvents,
    hydrationState: toolResultHydration,
    expandedThinkingMessages: transcriptExpansion.expandedThinkingMessages,
    expandedToolItems: transcriptExpansion.expandedToolItems,
  });
  const transcriptPagination = useTranscriptPaginationAnchor({
    messages,
    transcriptRef: scrollContainerRef,
    canLoadOlder: Boolean(currentSession && hasMoreHistory && nextBeforeMessageId),
    onLoadOlder: loadOlderHistory,
    onPrependCancelled: allowNextAutoScroll,
    onPrependStart: skipNextAutoScroll,
    onRestored: updateScrollThumb,
  });
  const handleTranscriptScroll = useCallback(() => {
    handleTranscriptScrollState();
    transcriptPagination.handleScroll();
  }, [handleTranscriptScrollState, transcriptPagination]);
  const resetTranscriptUi = useCallback(() => {
    transcriptExpansion.reset();
    resetTranscriptScroll();
    transcriptPagination.reset();
  }, [resetTranscriptScroll, transcriptExpansion, transcriptPagination]);
  const {
    expandedThinkingMessages,
    expandedToolItems,
    toggleThinkingMessage,
    toggleToolItem,
  } = transcriptExpansion;
  const abortControllerRef = useRef<AbortController | null>(null);
  const currentSessionRef = useRef<SessionTarget | null>(null);
  const resourceLoader = useCallback(
    (uri: string, signal: AbortSignal) => fetchResource
      ? fetchResource(uri, signal)
      : fetchResourceFromGateway({
          uri,
          gatewayClient,
          agent,
          sessionId: currentSession?.sessionId ?? sessionId ?? null,
          signal,
        }),
    [agent, currentSession?.sessionId, fetchResource, gatewayClient, sessionId],
  );
  const {
    openResourceUri,
    resourcePaneOpen,
    resourceView,
    resourceLoading,
    resourceError,
    open: openResource,
    close: closeResourcePane,
    reset: clearResourcePaneState,
    completeClose: handleResourcePaneExitComplete,
    abortRef: resourceAbortRef,
  } = useResourcePane(resourceLoader);
  const messagesRef = useRef<CopilotMessage[]>(emptyMessages);
  const submittedPreviewUrlsRef = useRef<string[]>([]);
  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);

  useEffect(() => {
    imageAttachmentsRef.current = imageAttachments;
  }, [imageAttachments]);

  useEffect(() => {
    currentSessionRef.current = currentSession;
  }, [currentSession]);

  useEffect(() => {
    if (!isSessionLive || loadingStartedAt === null) {
      return;
    }
    setLoadingNow(Date.now());
    const intervalId = window.setInterval(() => setLoadingNow(Date.now()), 250);
    return () => window.clearInterval(intervalId);
  }, [isSessionLive, loadingStartedAt]);

  useEffect(() => {
    return () => {
      abortControllerRef.current?.abort();
      for (const attachment of imageAttachmentsRef.current) {
        URL.revokeObjectURL(attachment.previewUrl);
      }
      for (const previewUrl of submittedPreviewUrlsRef.current) {
        URL.revokeObjectURL(previewUrl);
      }
    };
  }, []);

  const inputRows = useMemo(() => {
    let rowCount = 1;
    for (let index = 0; index < input.length; index += 1) {
      if (input.charCodeAt(index) === 10) {
        rowCount += 1;
      }
    }
    return Math.min(rowCount, 8);
  }, [input]);

  const updateSessionMessage = useCallback(
    async (message: CopilotMessage, parts: unknown[], labels: Record<string, string>) => {
      const session = currentSessionRef.current ?? (sessionId ? { ns: namespace, agent, sessionId } : null);
      const sessions = gatewayClient?.sessions;
      if (!session) {
        throw new Error("No active session to update.");
      }
      if (!sessions?.updateMessage) {
        throw new Error("Gateway client does not support sessions.updateMessage().");
      }
      const response = await sessions.updateMessage({
        ns: session.ns,
        agent: session.agent,
        sessionId: session.sessionId,
        messageId: message.id,
        parts,
        labels,
      });
      const updated = response?.message
        ? { ...message, ...normalizeRawSessionMessage(response.message) }
        : { ...message, parts, labels, content: getMessageContent({ ...message, parts }) };
      setMessages((current) => {
        const nextMessages = current.map((item) => item.id === message.id ? updated : item);
        messagesRef.current = nextMessages;
        return nextMessages;
      });
      return updated;
    },
    [agent, gatewayClient, namespace, sessionId],
  );

  const startEditingMessage = useCallback((message: CopilotMessage) => {
    setEditingMessageId(message.id);
    setEditingMessageValue(editableMessageContent(message));
  }, []);

  const cancelEditingMessage = useCallback(() => {
    setEditingMessageId(null);
    setEditingMessageValue("");
  }, []);

  const saveEditingMessage = useCallback(async (message: CopilotMessage) => {
    const nextContent = editingMessageValue.trim();
    if (!nextContent) {
      return;
    }
    setError(null);
    const shouldPersistSessionEdit =
      enableDebugMessageEditing &&
      (message.role === "user" || message.role === "assistant") &&
      !isLocalMessageId(message.id);
    try {
      if (shouldPersistSessionEdit) {
        setReviewActionMessageId(message.id);
        await updateSessionMessage(message, replaceMessageTextPart(message, nextContent), { ...(message.labels ?? {}) });
        cancelEditingMessage();
        return;
      }
      const handled = await onMessageEdit?.({
        message,
        nextContent,
        namespace,
        agent,
        sessionId: currentSessionRef.current?.sessionId ?? sessionId ?? null,
      });
      if (handled === false) {
        return;
      }
      setMessages((prev) => {
        const nextMessages = prev.map((item) => item.id === message.id ? messageWithEditedContent(item, nextContent) : item);
        messagesRef.current = nextMessages;
        return nextMessages;
      });
      cancelEditingMessage();
    } catch (err) {
      setError(err instanceof Error ? err : new Error(String(err)));
    } finally {
      setReviewActionMessageId(null);
    }
  }, [agent, cancelEditingMessage, editingMessageValue, enableDebugMessageEditing, namespace, onMessageEdit, sessionId, updateSessionMessage]);

  const updateConnectorDeliveryStatus = useCallback(
    async (message: CopilotMessage, status: string) => {
      const labels = {
        ...(message.labels ?? {}),
        [LABEL_CONNECTOR_DELIVERY_STATUS]: status,
      };
      delete labels[LABEL_CONNECTOR_DELIVERY_ERROR];
      setError(null);
      setReviewActionMessageId(message.id);
      try {
        await updateSessionMessage(message, messagePartsForSessionUpdate(message), labels);
      } catch (err) {
        setError(err instanceof Error ? err : new Error(String(err)));
      } finally {
        setReviewActionMessageId(null);
      }
    },
    [updateSessionMessage],
  );

  const handleResourceClick = useCallback(
    (uri: string) => {
      if (onResourceClickProp) {
        onResourceClickProp(uri);
        return;
      }

      const parsed = parseResourceUri(uri);
      if (!parsed) return;

      if (openResourceUri === parsed.uri && resourcePaneOpen) {
        closeResourcePane();
        return;
      }

      const canFetchArtifact =
        Boolean(fetchResource) || Boolean(gatewayClient?.artifacts?.readArtifact);
      const canFetchFile = Boolean(fetchResource) || Boolean(gatewayClient?.files?.readFile);
      const canOpen =
        (parsed.kind === "artifact" && canFetchArtifact) ||
        (parsed.kind === "file" && canFetchFile);

      if (!canOpen) {
        if (!missingResourceClientWarnedRef.current && typeof console !== "undefined") {
          missingResourceClientWarnedRef.current = true;
          console.warn(
            "[talon-chat] Resource link clicked but no artifacts/files client (or fetchResource) is available.",
          );
        }
        return;
      }

      void openResource(parsed.uri);
    },
    [
      closeResourcePane,
      fetchResource,
      gatewayClient,
      openResource,
      onResourceClickProp,
      openResourceUri,
      resourcePaneOpen,
    ],
  );

  // Reset open resource pane when the session identity changes.
  useEffect(() => {
    clearResourcePaneState();
  }, [agent, clearResourcePaneState, currentSession?.sessionId, namespace, sessionId]);

  const renderedMessages = messages.map((message, messageIndex) => (
    <SessionMessage
      key={message.id}
      message={message}
      messageIndex={messageIndex}
      messages={messages}
      isSessionLive={isSessionLive}
      loadingStartedAt={loadingStartedAt}
      loadingNow={loadingNow}
      objectUrlForRef={objectUrlForRef}
      allowEditing={allowMessageEditing}
      enableDebugEditing={enableDebugMessageEditing}
      editingMessageId={editingMessageId}
      editingMessageValue={editingMessageValue}
      reviewActionMessageId={reviewActionMessageId}
      expandedThinkingMessages={expandedThinkingMessages}
      expandedToolItems={expandedToolItems}
      hydrationState={toolResultHydration}
      resultFor={toolResultFor}
      onToggleThinking={toggleThinkingMessage}
      onToggleTool={toggleToolItem}
      onHydrateTool={(...args) => void hydrateToolResultForExpandedItem(...args)}
      onResourceClick={handleResourceClick}
      onEditingValueChange={setEditingMessageValue}
      onSaveEdit={(target) => void saveEditingMessage(target)}
      onCancelEdit={cancelEditingMessage}
      onStartEdit={startEditingMessage}
      onCopy={(target) => void copyMessageContent(target)}
      onUpdateConnectorDelivery={(target, status) => void updateConnectorDeliveryStatus(target, status)}
    />
  ));

  const resolvedHistoryPageSize = Math.max(
    1,
    Math.trunc(historyPageSize || historyMessageLimit || DEFAULT_HISTORY_PAGE_SIZE),
  );

  const refreshNewestSessionPage = useCallback(
    async (target: { ns: string; agent: string; sessionId: string }, signal?: AbortSignal) => {
      setStreamEvents([]);
      return refreshRuntime(target, signal);
    },
    [refreshRuntime],
  );

  const {
    cancelResume,
    isStoppingRef,
    reset: resetGeneration,
    startResume,
    stopGeneration,
  } = useSessionGeneration({
    client: gatewayClient.sessions,
    currentSession,
    currentSessionRef,
    messagesRef,
    serverState: sessionRuntimeState.serverState,
    isSessionLive,
    isStopping,
    submissionAbortControllerRef: abortControllerRef,
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
  });

  const previousSessionTargetRef = useRef<SessionTarget | null>(null);
  useEffect(() => {
    const previousTarget = previousSessionTargetRef.current;
    previousSessionTargetRef.current = currentSession;
    if (!previousTarget || (currentSession && isSameSession(previousTarget, currentSession))) return;
    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    setIsStopping(false);
    setIsLoading(false);
    setIsResuming(false);
    setLoadingStartedAt(null);
    setStreamEvents([]);
    resetTranscriptUi();
    invalidateToolResultHydration();
  }, [currentSession?.agent, currentSession?.ns, currentSession?.sessionId, invalidateToolResultHydration, resetTranscriptUi, setIsLoading, setIsResuming, setIsStopping]);

  const clearLocalSession = useCallback(() => {
    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    resetGeneration();
    resourceAbortRef.current?.abort();
    resourceAbortRef.current = null;
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
    clearResourcePaneState();
  }, [clearResourcePaneState, clearRuntime, invalidateToolResultHydration, resetGeneration, resetTranscriptUi]);

  const clearSession = useCallback(async () => {
    const session = currentSessionRef.current;
    if (session) {
      try {
        const sessions = gatewayClient?.sessions;
        if (!sessions?.clear) {
          throw new Error("TalonSession requires a Talon clientset with sessions.clear().");
        }
        await sessions.clear(session);
      } catch (err) {
        setError(err instanceof Error ? err : new Error(String(err)));
      }
    }
    clearLocalSession();
  }, [clearLocalSession, gatewayClient, setError]);

  const resolvedCommands = useMemo<Array<TalonSessionCommand>>(() => {
    const builtInCommands: TalonSessionCommand[] = [];
    if (enabledBuiltInCommands?.includes("clear")) {
      builtInCommands.push({
        name: "clear",
        description: "Clear the current session history.",
        run: ({ clear }) => clear?.(),
      });
    }
    if (enabledBuiltInCommands?.includes("goal")) {
      builtInCommands.push({
        name: "goal",
        description: "Create or update a session Goal.",
        run: () => undefined,
      });
    }
    return [...(commands ?? []), ...builtInCommands];
  }, [clearSession, commands, enabledBuiltInCommands]);
  const commandMenuItems = useMemo(
    () => resolvedCommands.map(({ name, aliases, description }) => ({ name, aliases, description })),
    [resolvedCommands],
  );
  const imageAccept = useMemo(() => acceptedImageTypes.join(","), [acceptedImageTypes]);
  const { submitMessage } = useSessionActions({
    client: gatewayClient.sessions,
    namespace,
    agent,
    sessionId,
    disabled,
    isSessionLive,
    enabledGoalCommand: Boolean(enabledBuiltInCommands?.includes("goal")),
    commands: resolvedCommands,
    onSessionChange,
    onSubmitMessage,
    currentSessionRef,
    messagesRef,
    imageAttachmentsRef,
    submissionAbortControllerRef: abortControllerRef,
    submittedPreviewUrlsRef,
    resolvedHistoryPageSize,
    setInput,
    setImageAttachments,
    setMessages,
    setStreamEvents,
    setError,
    setIsLoading,
    setIsResuming,
    setLoadingStartedAt,
    setLoadingNow,
    activateTarget,
    uploadQueuedImages,
    clearSession,
    cancelResume,
    startResume,
    isStoppingRef,
    markAutoScrollPinned,
    refreshRuntime,
    refreshNewestSessionPage,
  });

  runtimeSubmitRef.current = (input, context) => submitMessage(input.text, true, context.signal);
  runtimeStopRef.current = (context) => stopGeneration(context.signal);

  return (
    <div
      className={className}
      style={{
        display: "flex",
        flexDirection: "column",
        minWidth: 0,
        minHeight: 0,
        height: "100%",
        background: "transparent",
        color: "inherit",
        fontFamily: talonChatFontFamily,
        ...style,
      }}
    >
      <SessionStyles />
      <div
        style={{
          display: "flex",
          flexDirection: "row",
          flex: 1,
          minHeight: 0,
          minWidth: 0,
          position: "relative",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            // Fills remaining space so chat + pane land at an even 50/50 when open.
            flex: "1 1 auto",
            minWidth: 0,
            minHeight: 0,
            transition: "flex 280ms cubic-bezier(0.22, 1, 0.36, 1)",
          }}
        >
          <SessionTranscript
            isLive={isSessionLive}
            hasTrailingUserMessage={messages[messages.length - 1]?.role === "user"}
            workingLabel={formatWorkingDuration(loadingStartedAt, loadingNow)}
            error={error}
            incident={sessionState === "ERROR" && !error
              ? "This session previously encountered an error. You can continue, but any unavailable historical output will be marked in the transcript."
              : null}
            scrollThumb={scrollThumb}
            transcriptRef={scrollContainerRef}
            bottomRef={bottomRef}
            onScroll={handleTranscriptScroll}
          >
            {renderedMessages}
          </SessionTranscript>

          <SessionComposerDock
            disabled={disabled}
            value={input}
            onValueChange={setInput}
            onSubmit={(nextInput) => void (currentSession
              ? sessionRuntime.submit({ text: nextInput, imageAttachments })
              : submitMessage(nextInput))}
            placeholder={placeholder}
            variant={composerVariant}
            autoFocus={autoFocus}
            rows={inputRows}
            canSubmit={Boolean((input || "").trim() || imageAttachments.length > 0) && !isSessionLive}
            isGenerating={isSessionLive}
            canStop={Boolean(currentSession) && !isStopping}
            commandMenuItems={commandMenuItems}
            startAdornment={composerStartAdornment}
            endAdornment={composerEndAdornment}
            imageAttachments={imageAttachments.map((attachment) => ({
              id: attachment.id,
              filename: attachment.file.name,
              previewUrl: attachment.previewUrl,
              status: attachment.status,
              error: attachment.error,
            }))}
            imageUploadEnabled={Boolean(onImageUpload)}
            imageAccept={imageAccept}
            onImageFilesSelected={addImageFiles}
            onRemoveImageAttachment={removeImageAttachment}
            onStop={() => {
              void sessionRuntime.stop().catch((err: any) =>
                setError(err instanceof Error ? err : new Error("Failed to stop generation")),
              );
            }}
          />
        </div>

        {openResourceUri ? (
          <ResourcePane
            uri={openResourceUri}
            resource={resourceView}
            isLoading={resourceLoading}
            error={resourceError}
            open={resourcePaneOpen}
            onClose={closeResourcePane}
            onExitComplete={handleResourcePaneExitComplete}
            onResourceClick={handleResourceClick}
          />
        ) : null}
      </div>
    </div>
  );
}

export const TalonCopilot = TalonSession;
