import { useCallback } from "react";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import type { TalonClient } from "@impalasys/talon-client";
import {
  findTalonChatCommand,
  parseTalonChatCommandInput,
} from "../lib/commands";
import type { CopilotMessage } from "../lib/chatTimeline";
import type { StreamEventItem } from "../lib/uiStream";
import type { SessionHistoryPage } from "./history";
import type { SessionTarget } from "./types";
import {
  assistantSignature,
  createTurnController,
  finishSessionTurn,
  recoverFailedSessionTurn,
  sameSession,
  submitSessionTurn,
} from "./sessionSubmission";
import type {
  TalonSessionCommand,
  TalonSessionPendingImageAttachment,
  TalonSessionSubmitContext,
} from "./TalonSessionTypes";

type SessionActionsClient = Pick<TalonClient["sessions"], "create" | "submitTurn">;
type RefreshSession = (target: SessionTarget, signal?: AbortSignal) => Promise<SessionHistoryPage | null>;

type UseSessionActionsOptions = {
  client: SessionActionsClient | undefined;
  namespace: string;
  agent: string;
  sessionId?: string;
  disabled: boolean;
  isSessionLive: boolean;
  enabledGoalCommand: boolean;
  commands: TalonSessionCommand[];
  onSessionChange?: (sessionId: string) => void;
  onSubmitMessage?: (context: TalonSessionSubmitContext) => Promise<boolean | void> | boolean | void;
  currentSessionRef: MutableRefObject<SessionTarget | null>;
  messagesRef: MutableRefObject<CopilotMessage[]>;
  imageAttachmentsRef: MutableRefObject<TalonSessionPendingImageAttachment[]>;
  submissionAbortControllerRef: MutableRefObject<AbortController | null>;
  submittedPreviewUrlsRef: MutableRefObject<string[]>;
  resolvedHistoryPageSize: number;
  setInput: Dispatch<SetStateAction<string>>;
  setImageAttachments: Dispatch<SetStateAction<TalonSessionPendingImageAttachment[]>>;
  setMessages: Dispatch<SetStateAction<CopilotMessage[]>>;
  setStreamEvents: Dispatch<SetStateAction<StreamEventItem[]>>;
  setError: (error: Error | null) => void;
  setIsLoading: (value: boolean) => void;
  setIsResuming: (value: boolean) => void;
  setLoadingStartedAt: (value: string | number | null) => void;
  setLoadingNow: (value: number) => void;
  activateTarget: (target: SessionTarget, options?: { hydrate?: boolean }) => void;
  uploadQueuedImages: (target: SessionTarget, signal: AbortSignal) => Promise<TalonSessionPendingImageAttachment[]>;
  clearSession: () => Promise<void>;
  cancelResume: () => void;
  startResume: (target: SessionTarget) => void;
  isStoppingRef: MutableRefObject<boolean>;
  markAutoScrollPinned: () => void;
  refreshRuntime: RefreshSession;
  refreshNewestSessionPage: RefreshSession;
};

function prepareSubmission(text: string, enabledGoalCommand: boolean) {
  const parsedCommand = parseTalonChatCommandInput(text);
  if (parsedCommand?.name !== "goal" || !enabledGoalCommand) return { text, parsedCommand, error: null };
  const goalText = parsedCommand.args?.trim() ?? "";
  if (!goalText) return { text, parsedCommand: null, error: new Error("Usage: /goal <objective and success criteria>") };
  return {
    text: ["Create or update a Talon Goal for this session.", "", "Use the goal tools directly. Track this objective until completion:", goalText].join("\n"),
    parsedCommand: null,
    error: null,
  };
}

export function createLocalMessageId() {
  const timestamp = String(Date.now()).padStart(13, "0");
  const sequence = String(Math.floor(Math.random() * 1_000_000)).padStart(6, "0");
  let suffix = "000000";
  if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") {
    const bytes = new Uint8Array(3);
    crypto.getRandomValues(bytes);
    suffix = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  }
  return `local-${timestamp}-${sequence}-${suffix}`;
}

/** Coordinates command routing, session creation, optimistic user messages, and turn submission. */
export function useSessionActions({
  client,
  namespace,
  agent,
  sessionId,
  disabled,
  isSessionLive,
  enabledGoalCommand,
  commands,
  onSessionChange,
  onSubmitMessage,
  currentSessionRef,
  messagesRef,
  imageAttachmentsRef,
  submissionAbortControllerRef,
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
}: UseSessionActionsOptions) {
  const createSession = useCallback(async (): Promise<SessionTarget> => {
    if (!client?.create) throw new Error("TalonSession requires a Talon clientset with sessions.create().");
    const response = await client.create({ ns: namespace, agent });
    return { ns: namespace, agent, sessionId: response.sessionId };
  }, [agent, client, namespace]);

  const ensureSession = useCallback(async (): Promise<SessionTarget> => {
    let session = currentSessionRef.current;
    if (session) return session;
    session = sessionId ? { ns: namespace, agent, sessionId } : await createSession();
    currentSessionRef.current = session;
    activateTarget(session, { hydrate: false });
    if (!sessionId) onSessionChange?.(session.sessionId);
    return session;
  }, [activateTarget, agent, createSession, currentSessionRef, namespace, onSessionChange, sessionId]);

  const waitForCanonicalAssistantUpdate = useCallback(async (
    session: SessionTarget,
    baselineSignature: string,
    signal?: AbortSignal,
  ) => {
    for (let attempt = 0; attempt < 40; attempt += 1) {
      if (signal?.aborted || !sameSession(currentSessionRef.current, session)) return false;
      const state = await refreshRuntime(session, signal);
      if (signal?.aborted || !sameSession(currentSessionRef.current, session) || !state) return false;
      if (assistantSignature(state.messages) && assistantSignature(state.messages) !== baselineSignature) {
        await refreshNewestSessionPage(session, signal);
        return true;
      }
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    return false;
  }, [currentSessionRef, refreshNewestSessionPage, refreshRuntime]);

  const runHostSubmission = useCallback(async (text: string, attachments: TalonSessionPendingImageAttachment[]) => {
    if (!onSubmitMessage) return false;
    setError(null);
    try {
      return Boolean(await onSubmitMessage({
        text,
        namespace,
        agent,
        sessionId: currentSessionRef.current?.sessionId ?? sessionId ?? null,
        imageAttachments: attachments,
        ensureSession,
        clearInput: () => setInput(""),
        refreshSession: async () => {
          await refreshNewestSessionPage(await ensureSession());
        },
      }));
    } catch (error) {
      setError(error instanceof Error ? error : new Error(String(error)));
      return true;
    }
  }, [agent, currentSessionRef, ensureSession, namespace, onSubmitMessage, refreshNewestSessionPage, sessionId, setError, setInput]);

  const runSessionCommand = useCallback(async (
    text: string,
    parsedCommand: ReturnType<typeof parseTalonChatCommandInput>,
    hasImages: boolean,
  ) => {
    const command = findTalonChatCommand(commands, parsedCommand);
    if (!command || !parsedCommand || hasImages) return false;
    setInput("");
    setError(null);
    setStreamEvents([]);
    try {
      await command.run({
        name: parsedCommand.name,
        input: text,
        args: parsedCommand.args,
        argv: parsedCommand.argv,
        target: { type: "session", namespace, agent, sessionId: currentSessionRef.current?.sessionId ?? sessionId ?? null },
        messages: messagesRef.current,
        clear: clearSession,
      });
    } catch (error) {
      setError(error instanceof Error ? error : new Error(String(error)));
    }
    return true;
  }, [agent, clearSession, commands, currentSessionRef, messagesRef, namespace, sessionId, setError, setInput, setStreamEvents]);

  const submitSessionTurnAndRecover = useCallback(async (
    text: string,
    pendingAttachments: TalonSessionPendingImageAttachment[],
    submittedText: string,
    runtimeSignal?: AbortSignal,
  ) => {
    setError(null);
    setStreamEvents([]);
    cancelResume();
    setIsResuming(false);
    let turnStarted = false;
    let resumedAfterBusyFailure = false;
    let optimisticMessageId: string | null = null;
    let submittedSession: SessionTarget | null = null;
    let controller: AbortController | null = null;
    let removeRuntimeAbort = () => undefined;
    try {
      const baselineSignature = assistantSignature(messagesRef.current.slice(-resolvedHistoryPageSize));
      const session = await ensureSession();
      submittedSession = session;
      const turnController = createTurnController(runtimeSignal);
      controller = turnController.controller;
      removeRuntimeAbort = turnController.removeRuntimeAbort;
      submissionAbortControllerRef.current = controller;
      const { hasAssistantEvent } = await submitSessionTurn({
        client,
        controller,
        createMessageId: createLocalMessageId,
        markAutoScrollPinned,
        onOptimisticMessage: (id) => { optimisticMessageId = id; },
        onTurnStarted: () => { turnStarted = true; },
        pendingAttachments,
        session,
        setImageAttachments,
        setInput,
        setIsLoading,
        setLoadingNow,
        setLoadingStartedAt,
        setMessages,
        setStreamEvents,
        submittedPreviewUrlsRef,
        text,
        uploadQueuedImages,
      });
      if (!hasAssistantEvent) await waitForCanonicalAssistantUpdate(session, baselineSignature, controller.signal);
      else await refreshNewestSessionPage(session, controller.signal);
    } catch (err) {
      const nextError = err instanceof Error ? err : new Error(String(err));
      const session = submittedSession && sameSession(currentSessionRef.current, submittedSession) ? submittedSession : null;
      const recovery = await recoverFailedSessionTurn({
        controller,
        currentSessionRef,
        error: nextError,
        imageAttachmentsRef,
        isStoppingRef,
        messagesRef,
        optimisticMessageId,
        pendingAttachments,
        refreshNewestSessionPage,
        session,
        setError,
        setImageAttachments,
        setInput,
        setMessages,
        startResume,
        submittedText,
        resolvedHistoryPageSize,
        turnStarted,
        waitForCanonicalAssistantUpdate,
      });
      resumedAfterBusyFailure = recovery === "resumed";
      if (recovery === "unhandled") setError(nextError);
    } finally {
      removeRuntimeAbort();
      finishSessionTurn({
        controller,
        currentSessionRef,
        resumedAfterBusyFailure,
        session: submittedSession,
        setIsLoading,
        setLoadingStartedAt,
        submissionAbortControllerRef,
      });
    }
  }, [cancelResume, client, currentSessionRef, ensureSession, imageAttachmentsRef, isStoppingRef, markAutoScrollPinned, messagesRef, refreshNewestSessionPage, resolvedHistoryPageSize, setError, setImageAttachments, setInput, setIsLoading, setIsResuming, setLoadingNow, setLoadingStartedAt, setMessages, setStreamEvents, startResume, submissionAbortControllerRef, submittedPreviewUrlsRef, uploadQueuedImages, waitForCanonicalAssistantUpdate]);

  const submitMessage = useCallback(async (submittedText: string, invokedByRuntime = false, runtimeSignal?: AbortSignal) => {
    const initialText = submittedText.trim();
    const pendingAttachments = imageAttachmentsRef.current;
    const hasImages = pendingAttachments.length > 0;
    if ((!initialText && !hasImages) || (!invokedByRuntime && isSessionLive) || disabled) return;
    if (await runHostSubmission(initialText, pendingAttachments)) return;
    const prepared = prepareSubmission(initialText, enabledGoalCommand);
    if (prepared.error) {
      setError(prepared.error);
      return;
    }
    if (await runSessionCommand(prepared.text, prepared.parsedCommand, hasImages)) return;
    await submitSessionTurnAndRecover(prepared.text, pendingAttachments, submittedText, runtimeSignal);
  }, [disabled, enabledGoalCommand, imageAttachmentsRef, isSessionLive, runHostSubmission, runSessionCommand, setError, submitSessionTurnAndRecover]);

  return { submitMessage };
}
