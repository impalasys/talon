"use client";

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  hydrateMessagesWithSteps,
  normalizeMessageRole,
  type CopilotMessage,
} from "./lib/chatTimeline";
import { ResourcePane } from "./lib/ResourcePane";
import { type StreamEventItem } from "./lib/uiStream";
import { streamSessionPartEvents as parseSessionPartEvents } from "./session/stream";
import { SessionTranscript } from "./session/SessionTranscript";
import { SessionComposerDock } from "./session/SessionComposerDock";
import { useSessionAttachments } from "./session/hooks/useSessionAttachments";
import { useToolResultHydration } from "./session/hooks/useToolResultHydration";
import { useTranscriptExpansionState } from "./session/hooks/useTranscriptExpansionState";
import { useTranscriptPaginationAnchor } from "./session/hooks/useTranscriptPaginationAnchor";
import { useTranscriptScrollState } from "./session/hooks/useTranscriptScrollState";
import { parseResourceUri } from "./lib/resourceUris";
import { useSessionGeneration } from "./session/useSessionGeneration";
import { createLocalMessageId, useSessionActions } from "./session/useSessionActions";
import { useSessionLifecycle } from "./session/useSessionLifecycle";
import { copyMessageContent } from "./session/copyMessageContent";
import { formatWorkingDuration } from "./session/sessionTiming";
import { SessionStyles } from "./session/SessionStyles";
import { SessionMessageList } from "./session/SessionMessageList";
import {
  useSessionMessageEditing,
} from "./session/useSessionMessageEditing";
import { useSessionResourceClick } from "./session/useSessionResourceClick";
import { useTalonSessionRuntime } from "./session/useTalonSessionRuntime";
import { useSessionPresentationState } from "./session/useSessionPresentationState";
import { useSessionResources } from "./session/useSessionResources";
import { useSessionCommands } from "./session/useSessionCommands";
import { useSessionPendingMessages } from "./session/useSessionPendingMessages";
import { SessionArtifactsRail } from "./session/SessionArtifactsRail";
import { artifactUriFor, type SessionArtifact } from "./session/artifacts";
import { useSessionArtifacts } from "./session/hooks/useSessionArtifacts";
import type {
  TalonSessionProps,
} from "./session/TalonSessionTypes";
import {
  canCompareCanonicalMessageIds,
  mergeNewestCanonicalPage,
  normalizeHistoryPage,
  normalizeMessageLabels,
  type SessionHistoryPage,
} from "./session/history";

export type * from "./session/TalonSessionTypes";
export type { ResourceViewModel } from "./lib/resourceUris";

const emptyMessages: CopilotMessage[] = [];
const DEFAULT_HISTORY_PAGE_SIZE = 50;
const DEFAULT_HISTORY_MESSAGE_LIMIT = 100;
const DEFAULT_HISTORY_STEP_LIMIT = 1000;

const talonChatFontFamily =
  'var(--talon-chat-font-family, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif)';

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
  onAttachmentUpload,
  onImageUpload,
  objectUrlForRef,
  maxAttachments,
  maxAttachmentBytes,
  acceptedAttachmentTypes,
  maxImageAttachments,
  maxImageBytes,
  acceptedImageTypes,
  composerVariant = "panel",
  composerStartAdornment,
  composerEndAdornment,
  onSubmitMessage,
  submissionTransformer,
  onTurnComplete,
  messageDisplayTransformer,
  allowMessageEditing = false,
  onMessageEdit,
  enableDebugMessageEditing = false,
  showSessionArtifacts = false,
  onResourceClick: onResourceClickProp,
  fetchResource,
}: TalonSessionProps) {
  const {
    runtime: sessionRuntime,
    setIsLoading,
    setIsResuming,
    setIsStopping,
    setSessionState,
    submitRef: runtimeSubmitRef,
    stopRef: runtimeStopRef,
  } = useTalonSessionRuntime({
    agent,
    gatewayClient,
    historyPageSize,
    historyMessageLimit,
    namespace,
    sessionId,
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
  const displayMessages = useMemo(
    () => messageDisplayTransformer
      ? messages.map((message) => messageDisplayTransformer(message))
      : messages,
    [messageDisplayTransformer, messages],
  );
  const currentSession = sessionRuntimeState.target;
  const pendingMessages = useSessionPendingMessages(gatewayClient.sessions, currentSession);
  const isLoading = sessionRuntimeState.phase === "submitting";
  const isResuming = sessionRuntimeState.phase === "resuming";
  const isStopping = sessionRuntimeState.phase === "stopping";
  const sessionState = sessionRuntimeState.serverState === "UNKNOWN" ? null : sessionRuntimeState.serverState;
  const isSessionLive = runtimeIsLive;
  const [input, setInput] = useState("");
  const [sessionArtifactsDismissed, setSessionArtifactsDismissed] = useState(false);
  const resolvedMaxAttachments = maxAttachments ?? maxImageAttachments ?? 4;
  const resolvedMaxAttachmentBytes = maxAttachmentBytes ?? maxImageBytes ?? 20 * 1024 * 1024;
  const resolvedAcceptedAttachmentTypes = acceptedAttachmentTypes ?? acceptedImageTypes ?? ["image/png", "image/jpeg", "image/gif", "image/webp"];
  const {
    addFiles: addImageFiles,
    attachments: imageAttachments,
    attachmentsRef: imageAttachmentsRef,
    remove: removeImageAttachment,
    replace: setImageAttachments,
    uploadQueued: uploadQueuedImages,
  } = useSessionAttachments({
    acceptedTypes: resolvedAcceptedAttachmentTypes,
    createId: createLocalMessageId,
    maxAttachments: resolvedMaxAttachments,
    maxBytes: resolvedMaxAttachmentBytes,
    onError: setError,
    onUpload: onAttachmentUpload ?? onImageUpload,
  });
  const [loadingStartedAt, setLoadingStartedAt] = useState<string | number | null>(null);
  const error = sessionRuntimeState.error;
  const [streamEvents, setStreamEvents] = useState<StreamEventItem[]>([]);
  const [maintenanceNotice, setMaintenanceNotice] = useState<string | null>(null);
  const {
    state: toolResultHydration,
    resultFor: toolResultFor,
    hydrate: hydrateToolResultForExpandedItem,
    invalidate: invalidateToolResultHydration,
  } = useToolResultHydration(
    gatewayClient?.cas,
    currentSession ? `${currentSession.ns}\u0000${currentSession.agent}\u0000${currentSession.sessionId}` : null,
  );
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
  const {
    currentSessionRef,
    inputRows,
    loadingNow,
    messagesRef,
    setLoadingNow,
    submittedPreviewUrlsRef,
  } = useSessionPresentationState({
    abortControllerRef,
    currentSession,
    input,
    isSessionLive,
    loadingStartedAt,
    messages,
  });
  const {
    openResourceUri,
    resourcePaneOpen,
    resourceView,
    resourceLoading,
    resourceError,
    open: openResource,
    close: closeResourcePane,
    reset: clearResourcePaneState,
    completeClose: completeResourcePaneClose,
    abortRef: resourceAbortRef,
  } = useSessionResources({ agent, currentSessionId: currentSession?.sessionId ?? null, fetchResource, gatewayClient, sessionId });
  const sessionArtifacts = useSessionArtifacts({
    enabled: showSessionArtifacts,
    gatewayClient,
    target: currentSession,
  });
  const sessionArtifactScope = currentSession
    ? `${currentSession.ns}\u0000${currentSession.agent}\u0000${currentSession.sessionId}`
    : "";
  useEffect(() => {
    setSessionArtifactsDismissed(false);
  }, [sessionArtifactScope]);
  const wasSessionLiveRef = useRef(isSessionLive);
  useEffect(() => {
    if (wasSessionLiveRef.current && !isSessionLive) {
      void sessionArtifacts.refresh().then((latestArtifacts) => {
        const parsed = openResourceUri ? parseResourceUri(openResourceUri) : null;
        if (parsed?.kind !== "artifact" || resourceView?.kind !== "artifact" || !resourceView.objectKey) return;
        const latest = latestArtifacts?.find((artifact) => artifact.id === parsed.artifactId);
        if (latest?.objectKey && latest.objectKey !== resourceView.objectKey) {
          void openResource(parsed.uri);
        }
      });
    }
    wasSessionLiveRef.current = isSessionLive;
  }, [isSessionLive, openResource, openResourceUri, resourceView, sessionArtifacts.refresh]);
  const handleCloseResourcePane = useCallback(() => {
    closeResourcePane();
  }, [closeResourcePane]);
  const handleResourcePaneExitComplete = useCallback(() => {
    completeResourcePaneClose();
  }, [completeResourcePaneClose]);
  const handleSelectArtifact = useCallback((artifact: SessionArtifact) => {
    if (!currentSession) return;
    const artifactUri = artifactUriFor(currentSession, artifact.id);
    if (onResourceClickProp) {
      onResourceClickProp(artifactUri);
      return;
    }
    void openResource(artifactUri);
  }, [currentSession, onResourceClickProp, openResource]);
  const {
    editingMessageId,
    editingMessageValue,
    reviewActionMessageId,
    setEditingMessageValue,
    startEditingMessage,
    cancelEditingMessage,
    saveEditingMessage,
    updateConnectorDeliveryStatus,
  } = useSessionMessageEditing({
    agent,
    client: gatewayClient.sessions,
    currentSessionRef,
    enableDebugMessageEditing,
    fallbackSessionId: sessionId,
    messagesRef,
    namespace,
    onMessageEdit,
    setError,
    setMessages,
  });

  const handleResourceClick = useSessionResourceClick({
    canFetchArtifact: Boolean(fetchResource) || Boolean(gatewayClient.artifacts?.readArtifact),
    canFetchFile: Boolean(fetchResource) || Boolean(gatewayClient.files?.readFile),
    closeResourcePane: handleCloseResourcePane,
    onResourceClick: onResourceClickProp,
    openResource,
    openResourceUri,
    resourcePaneOpen,
  });

  const renderedMessages = (
    <SessionMessageList
      messages={displayMessages}
      messageProps={{
        isSessionLive,
        loadingStartedAt,
        loadingNow,
        objectUrlForRef,
        allowEditing: allowMessageEditing,
        enableDebugEditing: enableDebugMessageEditing,
        editingMessageId,
        editingMessageValue,
        reviewActionMessageId,
        expandedThinkingMessages,
        expandedToolItems,
        hydrationState: toolResultHydration,
        resultFor: toolResultFor,
        onToggleThinking: toggleThinkingMessage,
        onToggleTool: toggleToolItem,
        onHydrateTool: (...args) => void hydrateToolResultForExpandedItem(...args),
        onResourceClick: handleResourceClick,
        onEditingValueChange: setEditingMessageValue,
        onSaveEdit: (target) => void saveEditingMessage(target),
        onCancelEdit: cancelEditingMessage,
        onStartEdit: startEditingMessage,
        onCopy: (target) => void copyMessageContent(target),
        onUpdateConnectorDelivery: (target, status) => void updateConnectorDeliveryStatus(target, status),
      }}
    />
  );

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

  const { clearSession } = useSessionLifecycle({
    client: gatewayClient.sessions,
    currentSession,
    currentSessionRef,
    requestedSessionKey: `${namespace}\u0000${agent}\u0000${sessionId ?? ""}\u0000${currentSession?.sessionId ?? ""}`,
    submissionAbortControllerRef: abortControllerRef,
    resourceAbortControllerRef: resourceAbortRef,
    messagesRef,
    emptyMessages,
    clearRuntime,
    resetGeneration,
    resetTranscriptUi,
    invalidateToolResultHydration,
    resetResourcePane: clearResourcePaneState,
    setStreamEvents,
    setError,
    setIsLoading,
    setIsResuming,
    setIsStopping,
    setSessionState,
    setLoadingStartedAt,
  });

  const compactSession = useCallback(async () => {
    const session = currentSessionRef.current;
    if (!session) throw new Error("Cannot compact a session that has not been created.");
    if (!gatewayClient.sessions.compact) throw new Error("TalonSession requires a Talon clientset with sessions.compact().");
    setMaintenanceNotice(null);
    setError(null);
    setIsLoading(true);
    try {
      // Maintenance parts include an internal compaction marker. Consume the
      // stream for lifecycle/error handling but do not feed that marker into
      // the chat timeline as a synthetic assistant message.
      let compacted = false;
      for await (const event of parseSessionPartEvents(gatewayClient.sessions.compact(session))) {
        if (event.type === "stream-failed") throw event.error;
        if (event.type === "stream-completed") break;
        if (event.type === "assistant-part") {
          const partType = event.part.partType ?? event.part.part_type;
          compacted ||= partType === 13 || partType === "SESSION_MESSAGE_PART_TYPE_COMPACTION";
        }
      }
      await refreshNewestSessionPage(session);
      setMaintenanceNotice(compacted
        ? "Session history compacted; provider continuation was reset."
        : "History already minimal; provider continuation was reset.");
    } finally {
      setIsLoading(false);
    }
  }, [currentSessionRef, gatewayClient.sessions, refreshNewestSessionPage, setError, setIsLoading]);

  const doctorSession = useCallback(async () => {
    const session = currentSessionRef.current;
    if (!session) throw new Error("Cannot diagnose a session that has not been created.");
    if (!gatewayClient.sessions.doctor) throw new Error("TalonSession requires a Talon clientset with sessions.doctor().");
    setMaintenanceNotice(null);
    const result = await gatewayClient.sessions.doctor(session);
    await refreshNewestSessionPage(session);
    setMaintenanceNotice(result.providerContinuationReset
      ? `Session doctor reset the saved provider continuation${result.incompleteToolBatches ? ` and found ${result.incompleteToolBatches} incomplete tool batch(es)` : ""}.`
      : "Session doctor found no saved provider continuation to reset.");
  }, [currentSessionRef, gatewayClient.sessions, refreshNewestSessionPage]);

  const { commandMenuItems, resolvedCommands } = useSessionCommands({ clearSession, compactSession, doctorSession, commands, enabledBuiltInCommands });
  const attachmentAccept = resolvedAcceptedAttachmentTypes.join(",");
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
    submissionTransformer,
    onTurnComplete,
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
            notice={maintenanceNotice}
            scrollThumb={scrollThumb}
            transcriptRef={scrollContainerRef}
            bottomRef={bottomRef}
            onScroll={handleTranscriptScroll}
          >
            {renderedMessages}
          </SessionTranscript>

          <SessionComposerDock
            disabled={disabled}
            pendingMessages={pendingMessages}
            value={input}
            onValueChange={setInput}
            onSubmit={(nextInput) => void (currentSession
              ? sessionRuntime.submit({ text: nextInput, attachments: imageAttachments, imageAttachments })
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
            attachments={imageAttachments.map((attachment) => ({
              id: attachment.id,
              filename: attachment.file.name,
              previewUrl: attachment.previewUrl,
              mediaType: attachment.file.type,
              status: attachment.status,
              error: attachment.error,
            }))}
            attachmentUploadEnabled={Boolean(onAttachmentUpload ?? onImageUpload)}
            attachmentAccept={attachmentAccept}
            onAttachmentFilesSelected={addImageFiles}
            onRemoveAttachment={removeImageAttachment}
            onStop={() => {
              void sessionRuntime.stop().catch((err: any) =>
                setError(err instanceof Error ? err : new Error("Failed to stop generation")),
              );
            }}
          />
        </div>

        {sessionArtifacts.available && !sessionArtifactsDismissed && (sessionArtifacts.artifacts.length > 0 || sessionArtifacts.error) && !openResourceUri ? (
          <SessionArtifactsRail
            artifacts={sessionArtifacts.artifacts}
            error={sessionArtifacts.error}
            hasMore={sessionArtifacts.hasMore}
            isLoading={sessionArtifacts.isLoading}
            onLoadMore={() => void sessionArtifacts.loadMore()}
            onSelect={handleSelectArtifact}
            onDismiss={() => setSessionArtifactsDismissed(true)}
          />
        ) : null}

        {openResourceUri ? (
          <ResourcePane
            uri={openResourceUri}
            resource={resourceView}
            isLoading={resourceLoading}
            error={resourceError}
            open={resourcePaneOpen}
            onClose={handleCloseResourcePane}
            onExitComplete={handleResourcePaneExitComplete}
            onResourceClick={handleResourceClick}
          />
        ) : null}
      </div>
    </div>
  );
}

export const TalonCopilot = TalonSession;
