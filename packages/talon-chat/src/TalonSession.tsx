"use client";

import React, { useCallback, useRef, useState } from "react";
import {
  hydrateMessagesWithSteps,
  normalizeMessageRole,
  type CopilotMessage,
} from "./lib/chatTimeline";
import { ResourcePane } from "./lib/ResourcePane";
import { type StreamEventItem } from "./lib/uiStream";
import { SessionTranscript } from "./session/SessionTranscript";
import { SessionComposerDock } from "./session/SessionComposerDock";
import { useSessionAttachments } from "./session/hooks/useSessionAttachments";
import { useToolResultHydration } from "./session/hooks/useToolResultHydration";
import { useTranscriptExpansionState } from "./session/hooks/useTranscriptExpansionState";
import { useTranscriptPaginationAnchor } from "./session/hooks/useTranscriptPaginationAnchor";
import { useTranscriptScrollState } from "./session/hooks/useTranscriptScrollState";
import { fetchResourceFromGateway } from "./lib/resourceLoader";
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
  allowMessageEditing = false,
  onMessageEdit,
  enableDebugMessageEditing = false,
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
  const currentSession = sessionRuntimeState.target;
  const isLoading = sessionRuntimeState.phase === "submitting";
  const isResuming = sessionRuntimeState.phase === "resuming";
  const isStopping = sessionRuntimeState.phase === "stopping";
  const sessionState = sessionRuntimeState.serverState === "UNKNOWN" ? null : sessionRuntimeState.serverState;
  const isSessionLive = runtimeIsLive;
  const [input, setInput] = useState("");
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
    completeClose: handleResourcePaneExitComplete,
    abortRef: resourceAbortRef,
  } = useSessionResources({ agent, currentSessionId: currentSession?.sessionId ?? null, fetchResource, gatewayClient, sessionId });
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
    closeResourcePane,
    onResourceClick: onResourceClickProp,
    openResource,
    openResourceUri,
    resourcePaneOpen,
  });

  const renderedMessages = (
    <SessionMessageList
      messages={messages}
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

  const { commandMenuItems, resolvedCommands } = useSessionCommands({ clearSession, commands, enabledBuiltInCommands });
  const imageAccept = acceptedImageTypes.join(",");
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
