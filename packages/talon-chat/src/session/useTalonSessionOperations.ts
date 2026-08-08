import { useCallback } from "react";
import type { CopilotMessage } from "../lib/chatTimeline";
import { useSessionActions } from "./useSessionActions";
import { useSessionCommands } from "./useSessionCommands";
import { useSessionGeneration } from "./useSessionGeneration";
import { useSessionLifecycle } from "./useSessionLifecycle";
import type { TalonSessionProps } from "./TalonSessionTypes";
import { useTalonSessionConversation } from "./useTalonSessionConversation";
import { useTalonSessionInteractions } from "./useTalonSessionInteractions";
import { useTalonSessionRuntime } from "./useTalonSessionRuntime";
import type { SessionTarget } from "./types";

const emptyMessages: CopilotMessage[] = [];

type Options = {
  agent: string;
  commands: TalonSessionProps["commands"];
  controls: ReturnType<typeof useTalonSessionRuntime>;
  conversation: ReturnType<typeof useTalonSessionConversation>;
  currentSession: SessionTarget | null;
  disabled: boolean;
  enabledBuiltInCommands: TalonSessionProps["enabledBuiltInCommands"];
  gatewayClient: TalonSessionProps["gatewayClient"];
  interactions: ReturnType<typeof useTalonSessionInteractions>;
  namespace: string;
  onSessionChange: TalonSessionProps["onSessionChange"];
  onSubmitMessage: TalonSessionProps["onSubmitMessage"];
  sessionId?: string;
  historyPageSize: number;
  historyMessageLimit: number;
};

/** Owns transport actions once conversation and interaction state are established. */
export function useTalonSessionOperations(options: Options) {
  const { controls, conversation, currentSession, interactions } = options;
  const runtime = controls.runtime;
  const refreshNewest = useCallback(async (target: SessionTarget, signal?: AbortSignal) => {
    conversation.setStreamEvents([]);
    return runtime.refresh(target, signal);
  }, [conversation.setStreamEvents, runtime.refresh]);
  const generation = useSessionGeneration({
    client: options.gatewayClient.sessions, currentSession, currentSessionRef: conversation.presentation.currentSessionRef,
    messagesRef: conversation.presentation.messagesRef, serverState: runtime.state.serverState, isSessionLive: runtime.isLive,
    isStopping: runtime.state.phase === "stopping", submissionAbortControllerRef: conversation.abortControllerRef,
    setMessages: runtime.setMessages, setStreamEvents: conversation.setStreamEvents, setError: runtime.setError,
    setIsLoading: controls.setIsLoading, setIsResuming: controls.setIsResuming, setIsStopping: controls.setIsStopping,
    setLoadingStartedAt: conversation.setLoadingStartedAt, setLoadingNow: conversation.presentation.setLoadingNow,
    refreshRuntime: runtime.refresh, refreshNewestSessionPage: refreshNewest,
  });
  const { clearSession } = useSessionLifecycle({
    client: options.gatewayClient.sessions, currentSession, currentSessionRef: conversation.presentation.currentSessionRef,
    requestedSessionKey: `${options.namespace}\u0000${options.agent}\u0000${options.sessionId ?? ""}\u0000${currentSession?.sessionId ?? ""}`,
    submissionAbortControllerRef: conversation.abortControllerRef, resourceAbortControllerRef: interactions.resources.abortRef,
    messagesRef: conversation.presentation.messagesRef, emptyMessages, clearRuntime: runtime.clear, resetGeneration: generation.reset,
    resetTranscriptUi: conversation.transcript.reset, invalidateToolResultHydration: conversation.hydration.invalidate,
    resetResourcePane: interactions.resources.reset, setStreamEvents: conversation.setStreamEvents, setError: runtime.setError,
    setIsLoading: controls.setIsLoading, setIsResuming: controls.setIsResuming, setIsStopping: controls.setIsStopping,
    setSessionState: controls.setSessionState, setLoadingStartedAt: conversation.setLoadingStartedAt,
  });
  const { commandMenuItems, resolvedCommands } = useSessionCommands({ clearSession, commands: options.commands, enabledBuiltInCommands: options.enabledBuiltInCommands });
  const { submitMessage } = useSessionActions({
    client: options.gatewayClient.sessions, namespace: options.namespace, agent: options.agent, sessionId: options.sessionId,
    disabled: options.disabled, isSessionLive: runtime.isLive, enabledGoalCommand: Boolean(options.enabledBuiltInCommands?.includes("goal")),
    commands: resolvedCommands, onSessionChange: options.onSessionChange, onSubmitMessage: options.onSubmitMessage,
    currentSessionRef: conversation.presentation.currentSessionRef, messagesRef: conversation.presentation.messagesRef,
    imageAttachmentsRef: conversation.images.attachmentsRef, submissionAbortControllerRef: conversation.abortControllerRef,
    submittedPreviewUrlsRef: conversation.presentation.submittedPreviewUrlsRef,
    resolvedHistoryPageSize: Math.max(1, Math.trunc(options.historyPageSize || options.historyMessageLimit || 50)),
    setInput: conversation.setInput, setImageAttachments: conversation.images.replace, setMessages: runtime.setMessages,
    setStreamEvents: conversation.setStreamEvents, setError: runtime.setError, setIsLoading: controls.setIsLoading,
    setIsResuming: controls.setIsResuming, setLoadingStartedAt: conversation.setLoadingStartedAt,
    setLoadingNow: conversation.presentation.setLoadingNow, activateTarget: runtime.activateTarget,
    uploadQueuedImages: conversation.images.uploadQueued, clearSession, cancelResume: generation.cancelResume,
    startResume: generation.startResume, isStoppingRef: generation.isStoppingRef,
    markAutoScrollPinned: conversation.transcript.markAutoScrollPinned, refreshRuntime: runtime.refresh, refreshNewestSessionPage: refreshNewest,
  });
  controls.submitRef.current = (input, context) => submitMessage(input.text, true, context.signal);
  controls.stopRef.current = (context) => generation.stopGeneration(context.signal);
  return { commandMenuItems, generation, submitMessage };
}
