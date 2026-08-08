import { createTalonSessionPresentationModel } from "./createTalonSessionPresentationModel";
import { useTalonSessionInteractions } from "./useTalonSessionInteractions";
import { useTalonSessionRuntime } from "./useTalonSessionRuntime";
import { useTalonSessionConversation } from "./useTalonSessionConversation";
import { useTalonSessionOperations } from "./useTalonSessionOperations";
import type { TalonSessionProps } from "./TalonSessionTypes";
import type { TalonSessionViewProps } from "./TalonSessionView";

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
  const { presentation } = conversation;
  const { currentSessionRef, messagesRef } = presentation;
  const interactions = useTalonSessionInteractions({
    agent, currentSession, currentSessionRef, enableDebugMessageEditing, fetchResource, gatewayClient,
    messagesRef, namespace, onMessageEdit, onResourceClick: onResourceClickProp, sessionId, setError, setMessages,
  });
  const operations = useTalonSessionOperations({
    agent, commands, controls: { runtime: sessionRuntime, setIsLoading, setIsResuming, setIsStopping, setSessionState, submitRef: runtimeSubmitRef, stopRef: runtimeStopRef },
    conversation, currentSession, disabled, enabledBuiltInCommands, gatewayClient, interactions, namespace,
    onSessionChange, onSubmitMessage, sessionId, historyPageSize, historyMessageLimit,
  });
  return createTalonSessionPresentationModel({
    acceptedAttachmentTypes, acceptedImageTypes, allowMessageEditing, autoFocus, className, composerEndAdornment, composerStartAdornment,
    composerVariant, conversation, currentSession, disabled, enableDebugMessageEditing, interactions, onAttachmentUpload, onImageUpload,
    isSessionLive, isStopping, objectUrlForRef, operations, placeholder, runtime: sessionRuntime, style,
  });
}
