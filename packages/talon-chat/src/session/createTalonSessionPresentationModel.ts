import type { SessionComposerDockProps } from "./SessionComposerDock";
import type { SessionMessageListProps } from "./SessionMessageList";
import type { SessionTranscriptProps } from "./SessionTranscript";
import type { TalonSessionProps } from "./TalonSessionTypes";
import type { TalonSessionViewProps } from "./TalonSessionView";
import { copyMessageContent } from "./copyMessageContent";
import { formatWorkingDuration } from "./sessionTiming";
import { useTalonSessionConversation } from "./useTalonSessionConversation";
import { useTalonSessionInteractions } from "./useTalonSessionInteractions";
import { useTalonSessionOperations } from "./useTalonSessionOperations";
import { useTalonSessionRuntime } from "./useTalonSessionRuntime";
import type { SessionTarget } from "./types";

type Options = Pick<
  TalonSessionProps,
  | "acceptedAttachmentTypes"
  | "acceptedImageTypes"
  | "allowMessageEditing"
  | "autoFocus"
  | "className"
  | "composerEndAdornment"
  | "composerStartAdornment"
  | "composerVariant"
  | "disabled"
  | "enableDebugMessageEditing"
  | "objectUrlForRef"
  | "onAttachmentUpload"
  | "onImageUpload"
  | "placeholder"
  | "style"
> & {
  conversation: ReturnType<typeof useTalonSessionConversation>;
  currentSession: SessionTarget | null;
  interactions: ReturnType<typeof useTalonSessionInteractions>;
  isSessionLive: boolean;
  isStopping: boolean;
  operations: ReturnType<typeof useTalonSessionOperations>;
  runtime: ReturnType<typeof useTalonSessionRuntime>["runtime"];
};

/** Builds the view-only props from the independently owned session hook state. */
export function createTalonSessionPresentationModel(options: Options): TalonSessionViewProps {
  return {
    className: options.className,
    style: options.style,
    messageList: createMessageListProps(options),
    transcript: createTranscriptProps(options),
    composer: createComposerProps(options),
    resourcePane: createResourcePaneProps(options),
  };
}

function createMessageListProps(options: Options): SessionMessageListProps {
  const { conversation, interactions, isSessionLive, runtime } = options;
  const { editing, handleResourceClick } = interactions;
  const { hydration, loadingStartedAt, presentation, transcript } = conversation;
  return {
    messages: runtime.state.messages,
    messageProps: {
      isSessionLive,
      loadingStartedAt,
      loadingNow: presentation.loadingNow,
      objectUrlForRef: options.objectUrlForRef,
      allowEditing: options.allowMessageEditing,
      enableDebugEditing: options.enableDebugMessageEditing,
      editingMessageId: editing.editingMessageId,
      editingMessageValue: editing.editingMessageValue,
      reviewActionMessageId: editing.reviewActionMessageId,
      expandedThinkingMessages: transcript.expandedThinkingMessages,
      expandedToolItems: transcript.expandedToolItems,
      hydrationState: hydration.state,
      resultFor: hydration.resultFor,
      onToggleThinking: transcript.toggleThinkingMessage,
      onToggleTool: transcript.toggleToolItem,
      onHydrateTool: (...args) => void hydration.hydrate(...args),
      onResourceClick: handleResourceClick,
      onEditingValueChange: editing.setEditingMessageValue,
      onSaveEdit: (message) => void editing.saveEditingMessage(message),
      onCancelEdit: editing.cancelEditingMessage,
      onStartEdit: editing.startEditingMessage,
      onCopy: (message) => void copyMessageContent(message),
      onUpdateConnectorDelivery: (message, status) => void editing.updateConnectorDeliveryStatus(message, status),
    },
  };
}

function createTranscriptProps(options: Options): Omit<SessionTranscriptProps, "children"> {
  const { conversation, isSessionLive, runtime } = options;
  const { loadingStartedAt, presentation, transcript } = conversation;
  const { state } = runtime;
  return {
    isLive: isSessionLive,
    hasTrailingUserMessage: state.messages[state.messages.length - 1]?.role === "user",
    workingLabel: formatWorkingDuration(loadingStartedAt, presentation.loadingNow),
    error: state.error,
    incident: state.serverState === "ERROR" && !state.error
      ? "This session previously encountered an error. You can continue, but any unavailable historical output will be marked in the transcript."
      : null,
    scrollThumb: transcript.scrollThumb,
    transcriptRef: transcript.transcriptRef,
    bottomRef: transcript.bottomRef,
    onScroll: transcript.handleScroll,
  };
}

function createComposerProps(options: Options): SessionComposerDockProps {
  const { conversation, currentSession, isSessionLive, isStopping, operations, runtime } = options;
  const { images, input, presentation, setInput } = conversation;
  return {
    disabled: options.disabled,
    value: input,
    onValueChange: setInput,
    onSubmit: (nextInput) => void (currentSession
      ? runtime.submit({ text: nextInput, attachments: images.attachments, imageAttachments: images.attachments })
      : operations.submitMessage(nextInput)),
    placeholder: options.placeholder,
    variant: options.composerVariant,
    autoFocus: options.autoFocus,
    rows: presentation.inputRows,
    canSubmit: Boolean(input.trim() || images.attachments.length > 0) && !isSessionLive,
    isGenerating: isSessionLive,
    canStop: Boolean(currentSession) && !isStopping,
    commandMenuItems: operations.commandMenuItems,
    startAdornment: options.composerStartAdornment,
    endAdornment: options.composerEndAdornment,
    attachments: images.attachments.map((attachment) => ({
      id: attachment.id,
      filename: attachment.file.name,
      previewUrl: attachment.previewUrl,
      mediaType: attachment.file.type,
      status: attachment.status,
      error: attachment.error,
    })),
    attachmentUploadEnabled: Boolean(options.onAttachmentUpload ?? options.onImageUpload),
    attachmentAccept: (options.acceptedAttachmentTypes ?? options.acceptedImageTypes ?? ["image/png", "image/jpeg", "image/gif", "image/webp"]).join(","),
    onAttachmentFilesSelected: images.addFiles,
    onRemoveAttachment: images.remove,
    onStop: () => {
      void runtime.stop().catch((error: unknown) => {
        runtime.setError(error instanceof Error ? error : new Error("Failed to stop generation"));
      });
    },
  };
}

function createResourcePaneProps(options: Options): TalonSessionViewProps["resourcePane"] {
  const { handleResourceClick, resources } = options.interactions;
  if (!resources.openResourceUri) return null;
  return {
    uri: resources.openResourceUri,
    resource: resources.resourceView,
    isLoading: resources.resourceLoading,
    error: resources.resourceError,
    open: resources.resourcePaneOpen,
    onClose: resources.close,
    onExitComplete: resources.completeClose,
    onResourceClick: handleResourceClick,
  };
}
