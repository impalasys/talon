import { useCallback } from "react";
import type { CopilotMessage } from "../lib/chatTimeline";
import { copyMessageContent } from "./copyMessageContent";
import { useSessionActions } from "./useSessionActions";
import { useSessionCommands } from "./useSessionCommands";
import { useSessionGeneration } from "./useSessionGeneration";
import { useSessionLifecycle } from "./useSessionLifecycle";
import { useTalonSessionInteractions } from "./useTalonSessionInteractions";
import { useTalonSessionRuntime } from "./useTalonSessionRuntime";
import { useTalonSessionConversation } from "./useTalonSessionConversation";
import { formatWorkingDuration } from "./sessionTiming";
import type { TalonSessionProps } from "./TalonSessionTypes";
import type { TalonSessionViewProps } from "./TalonSessionView";

const emptyMessages: CopilotMessage[] = [];
const DEFAULT_HISTORY_PAGE_SIZE = 50;
const DEFAULT_HISTORY_MESSAGE_LIMIT = 100;

/** Wires session transport and interaction hooks into the presentation model consumed by TalonSessionView. */
export function useTalonSessionController({
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
}: TalonSessionProps): TalonSessionViewProps {
  const {
    runtime: sessionRuntime,
    setIsLoading,
    setIsResuming,
    setIsStopping,
    setSessionState,
    submitRef: runtimeSubmitRef,
    stopRef: runtimeStopRef,
  } = useTalonSessionRuntime({ agent, gatewayClient, historyPageSize, historyMessageLimit, namespace, sessionId });
  const {
    state: sessionRuntimeState,
    isLive: isSessionLive,
    setMessages,
    setError,
    refresh: refreshRuntime,
    loadOlder: loadOlderRuntime,
    clear: clearRuntime,
    activateTarget,
  } = sessionRuntime;
  const messages = sessionRuntimeState.messages;
  const currentSession = sessionRuntimeState.target;
  const isStopping = sessionRuntimeState.phase === "stopping";
  const error = sessionRuntimeState.error;
  const conversation = useTalonSessionConversation({
    acceptedAttachmentTypes, acceptedImageTypes, agent, currentSession, error, gatewayClient, history: sessionRuntimeState.history,
    isSessionLive, loadOlderRuntime, maxAttachments, maxAttachmentBytes, maxImageAttachments, maxImageBytes, messages, onAttachmentUpload, onImageUpload, setError,
  });
  const { abortControllerRef, hydration, images, input, loadingStartedAt, presentation, setInput, setLoadingStartedAt, setStreamEvents, streamEvents, transcript } = conversation;
  const { attachments: imageAttachments, attachmentsRef: imageAttachmentsRef, addFiles: addImageFiles, remove: removeImageAttachment, replace: setImageAttachments, uploadQueued: uploadQueuedImages } = images;
  const { currentSessionRef, inputRows, loadingNow, messagesRef, setLoadingNow, submittedPreviewUrlsRef } = presentation;
  const { state: toolResultHydration, resultFor: toolResultFor, hydrate: hydrateToolResultForExpandedItem, invalidate: invalidateToolResultHydration } = hydration;
  const { bottomRef, expandedThinkingMessages, expandedToolItems, handleScroll: handleTranscriptScroll, markAutoScrollPinned, reset: resetTranscriptUi, scrollThumb, toggleThinkingMessage, toggleToolItem, transcriptRef: scrollContainerRef } = transcript;
  const interactions = useTalonSessionInteractions({
    agent, currentSession, currentSessionRef, enableDebugMessageEditing, fetchResource, gatewayClient,
    messagesRef, namespace, onMessageEdit, onResourceClick: onResourceClickProp, sessionId, setError, setMessages,
  });
  const { editing, handleResourceClick, resources } = interactions;
  const { openResourceUri, resourcePaneOpen, resourceView, resourceLoading, resourceError, close: closeResourcePane, reset: clearResourcePaneState, completeClose: handleResourcePaneExitComplete, abortRef: resourceAbortRef } = resources;
  const resolvedHistoryPageSize = Math.max(1, Math.trunc(historyPageSize || historyMessageLimit || DEFAULT_HISTORY_PAGE_SIZE));
  const refreshNewestSessionPage = useCallback(async (target: { ns: string; agent: string; sessionId: string }, signal?: AbortSignal) => {
    setStreamEvents([]);
    return refreshRuntime(target, signal);
  }, [refreshRuntime]);
  const generation = useSessionGeneration({
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
    resetGeneration: generation.reset,
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
    cancelResume: generation.cancelResume,
    startResume: generation.startResume,
    isStoppingRef: generation.isStoppingRef,
    markAutoScrollPinned,
    refreshRuntime,
    refreshNewestSessionPage,
  });
  runtimeSubmitRef.current = (submission, context) => submitMessage(submission.text, true, context.signal);
  runtimeStopRef.current = (context) => generation.stopGeneration(context.signal);

  return {
    className,
    style,
    messageList: {
      messages,
      messageProps: {
        isSessionLive,
        loadingStartedAt,
        loadingNow,
        objectUrlForRef,
        allowEditing: allowMessageEditing,
        enableDebugEditing: enableDebugMessageEditing,
        editingMessageId: editing.editingMessageId,
        editingMessageValue: editing.editingMessageValue,
        reviewActionMessageId: editing.reviewActionMessageId,
        expandedThinkingMessages,
        expandedToolItems,
        hydrationState: toolResultHydration,
        resultFor: toolResultFor,
        onToggleThinking: toggleThinkingMessage,
        onToggleTool: toggleToolItem,
        onHydrateTool: (...args) => void hydrateToolResultForExpandedItem(...args),
        onResourceClick: handleResourceClick,
        onEditingValueChange: editing.setEditingMessageValue,
        onSaveEdit: (message) => void editing.saveEditingMessage(message),
        onCancelEdit: editing.cancelEditingMessage,
        onStartEdit: editing.startEditingMessage,
        onCopy: (message) => void copyMessageContent(message),
        onUpdateConnectorDelivery: (message, status) => void editing.updateConnectorDeliveryStatus(message, status),
      },
    },
    transcript: {
      isLive: isSessionLive,
      hasTrailingUserMessage: messages[messages.length - 1]?.role === "user",
      workingLabel: formatWorkingDuration(loadingStartedAt, loadingNow),
      error,
      incident: sessionRuntimeState.serverState === "ERROR" && !error
        ? "This session previously encountered an error. You can continue, but any unavailable historical output will be marked in the transcript."
        : null,
      scrollThumb,
      transcriptRef: scrollContainerRef,
      bottomRef,
      onScroll: handleTranscriptScroll,
    },
    composer: {
      disabled,
      value: input,
      onValueChange: setInput,
      onSubmit: (nextInput) => void (currentSession
        ? sessionRuntime.submit({ text: nextInput, attachments: imageAttachments, imageAttachments })
        : submitMessage(nextInput)),
      placeholder,
      variant: composerVariant,
      autoFocus,
      rows: inputRows,
      canSubmit: Boolean(input.trim() || imageAttachments.length > 0) && !isSessionLive,
      isGenerating: isSessionLive,
      canStop: Boolean(currentSession) && !isStopping,
      commandMenuItems,
      startAdornment: composerStartAdornment,
      endAdornment: composerEndAdornment,
      attachments: imageAttachments.map((attachment) => ({
        id: attachment.id,
        filename: attachment.file.name,
        previewUrl: attachment.previewUrl,
        mediaType: attachment.file.type,
        status: attachment.status,
        error: attachment.error,
      })),
      attachmentUploadEnabled: Boolean(onAttachmentUpload ?? onImageUpload),
      attachmentAccept: (acceptedAttachmentTypes ?? acceptedImageTypes ?? ["image/png", "image/jpeg", "image/gif", "image/webp"]).join(","),
      onAttachmentFilesSelected: addImageFiles,
      onRemoveAttachment: removeImageAttachment,
      onStop: () => { void sessionRuntime.stop().catch((stopError: unknown) => setError(stopError instanceof Error ? stopError : new Error("Failed to stop generation"))); },
    },
    resourcePane: openResourceUri ? {
      uri: openResourceUri,
      resource: resourceView,
      isLoading: resourceLoading,
      error: resourceError,
      open: resourcePaneOpen,
      onClose: closeResourcePane,
      onExitComplete: handleResourcePaneExitComplete,
      onResourceClick: handleResourceClick,
    } : null,
  };
}
