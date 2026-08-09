import { useCallback } from "react";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import { data, type TalonClient } from "@impalasys/talon-client";
import {
  findTalonChatCommand,
  parseTalonChatCommandInput,
} from "../lib/commands";
import { getMessageContent, type CopilotMessage } from "../lib/chatTimeline";
import { streamSessionPartEvents, type StreamEventItem } from "../lib/uiStream";
import { normalizeObjectRefForJson, objectRefMediaType } from "./objectRefs";
import { protoSessionPartsFromChatParts } from "./protocol";
import type { SessionHistoryPage } from "./history";
import type { SessionTarget } from "./types";
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

function sameSession(left: SessionTarget | null, right: SessionTarget | null) {
  return left?.ns === right?.ns && left?.agent === right?.agent && left?.sessionId === right?.sessionId;
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

function normalizeEpochToMilliseconds(value: unknown) {
  const numericValue = typeof value === "string" ? Number(value) : value;
  const normalized = typeof numericValue === "number" && Number.isFinite(numericValue) ? numericValue : null;
  if (!normalized || normalized <= 0) return null;
  if (normalized >= 1e15) return Math.trunc(normalized / 1000);
  if (normalized >= 1e12) return Math.trunc(normalized);
  if (normalized >= 1e9) return Math.trunc(normalized * 1000);
  return null;
}

function assistantSignature(messages: CopilotMessage[]) {
  return messages
    .filter((message) => message.role === "assistant")
    .map((message) => `${message.id}:${getMessageContent(message).length}`)
    .join("|");
}

function isBusyError(error: unknown) {
  const candidate = error as { message?: unknown } | null;
  return typeof candidate?.message === "string" && /session is currently generating|session is busy/i.test(candidate.message);
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

  const submitMessage = useCallback(async (submittedText: string, invokedByRuntime = false, runtimeSignal?: AbortSignal) => {
    let text = submittedText.trim();
    const pendingAttachments = imageAttachmentsRef.current;
    const hasImages = pendingAttachments.length > 0;
    if ((!text && !hasImages) || (!invokedByRuntime && isSessionLive) || disabled) return;

    if (onSubmitMessage) {
      setError(null);
      try {
        const handled = await onSubmitMessage({
          text,
          namespace,
          agent,
          sessionId: currentSessionRef.current?.sessionId ?? sessionId ?? null,
          attachments: pendingAttachments,
          imageAttachments: pendingAttachments,
          ensureSession,
          clearInput: () => setInput(""),
          refreshSession: async () => {
            await refreshNewestSessionPage(await ensureSession());
          },
        });
        if (handled) return;
      } catch (err) {
        setError(err instanceof Error ? err : new Error(String(err)));
        return;
      }
    }

    let parsedCommand = parseTalonChatCommandInput(text);
    if (parsedCommand?.name === "goal" && enabledGoalCommand) {
      const goalText = parsedCommand.args?.trim() ?? "";
      if (!goalText) {
        setError(new Error("Usage: /goal <objective and success criteria>"));
        return;
      }
      text = ["Create or update a Talon Goal for this session.", "", "Use the goal tools directly. Track this objective until completion:", goalText].join("\n");
      parsedCommand = null;
    }
    const command = findTalonChatCommand(commands, parsedCommand);
    if (command && parsedCommand && !hasImages) {
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
      } catch (err) {
        setError(err instanceof Error ? err : new Error(String(err)));
      }
      return;
    }

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
      controller = new AbortController();
      const abortFromRuntime = () => controller?.abort();
      if (runtimeSignal) {
        if (runtimeSignal.aborted) controller.abort();
        else runtimeSignal.addEventListener("abort", abortFromRuntime, { once: true });
      }
      removeRuntimeAbort = () => runtimeSignal?.removeEventListener("abort", abortFromRuntime);
      submissionAbortControllerRef.current = controller;
      const uploadedImages = await uploadQueuedImages(session, controller.signal);
      const imageParts = uploadedImages.map((attachment) => {
        if (!attachment.object) throw new Error(`Image ${attachment.file.name} was not uploaded.`);
        return {
          type: "image",
          object: normalizeObjectRefForJson({
            ...attachment.object,
            filename: attachment.object.filename || attachment.file.name,
            mediaType: objectRefMediaType(attachment.object) || attachment.file.type,
            sizeBytes: attachment.object.sizeBytes ?? attachment.object.size_bytes ?? attachment.file.size,
          }),
          previewUrl: attachment.previewUrl,
          payloadJson: JSON.stringify({ filename: attachment.file.name }),
        };
      });
      const userMessage: CopilotMessage = {
        id: createLocalMessageId(),
        role: "user",
        content: text,
        parts: [...(text ? [{ type: "text", text }] : []), ...imageParts],
        createdAt: String(Date.now() * 1000),
      };
      optimisticMessageId = userMessage.id;
      setInput("");
      submittedPreviewUrlsRef.current.push(...uploadedImages.map((attachment) => attachment.previewUrl));
      setImageAttachments([]);
      setMessages((previous) => [...previous, userMessage]);
      setLoadingStartedAt(normalizeEpochToMilliseconds(userMessage.createdAt) ?? Date.now());
      setLoadingNow(Date.now());
      markAutoScrollPinned();
      setIsLoading(true);
      if (!client?.submitTurn) throw new Error("TalonSession requires a Talon clientset with sessions.submitTurn().");
      turnStarted = true;
      const { hasAssistantEvent } = await streamSessionPartEvents({
        events: client.submitTurn({
          ns: session.ns,
          agent: session.agent,
          sessionId: session.sessionId,
          message: { role: data.MessageRole.ROLE_USER, parts: protoSessionPartsFromChatParts(userMessage.parts) },
          labels: {},
        }, { signal: controller.signal }),
        setMessages,
        setStreamEvents,
        signal: controller.signal,
      });
      if (!hasAssistantEvent) await waitForCanonicalAssistantUpdate(session, baselineSignature, controller.signal);
      else await refreshNewestSessionPage(session, controller.signal);
    } catch (err) {
      const nextError = err instanceof Error ? err : new Error(String(err));
      const session = submittedSession && sameSession(currentSessionRef.current, submittedSession) ? submittedSession : null;
      if (controller?.signal.aborted || (submittedSession && !session)) return;
      if (session && isBusyError(nextError)) {
        if (optimisticMessageId) {
          messagesRef.current = messagesRef.current.filter((message) => message.id !== optimisticMessageId);
          setMessages((previous) => previous.filter((message) => message.id !== optimisticMessageId));
          setInput((current) => current || submittedText.trim());
          if (imageAttachmentsRef.current.length === 0 && pendingAttachments.length > 0) {
            imageAttachmentsRef.current = pendingAttachments;
            setImageAttachments(pendingAttachments);
          }
        }
        const refreshed = await refreshNewestSessionPage(session, controller?.signal).catch(() => null);
        if (controller?.signal.aborted || !sameSession(currentSessionRef.current, session) || isStoppingRef.current) return;
        if (refreshed?.state === "PROCESSING") {
          resumedAfterBusyFailure = true;
          setError(null);
          startResume(session);
          return;
        }
      }
      if (session && turnStarted && !isBusyError(nextError)) {
        const baselineSignature = assistantSignature(messagesRef.current.slice(-resolvedHistoryPageSize));
        if (await waitForCanonicalAssistantUpdate(session, baselineSignature, controller?.signal).catch(() => false)) {
          setError(null);
          return;
        }
      }
      setError(nextError);
    } finally {
      const stale = controller?.signal.aborted || (submittedSession && !sameSession(currentSessionRef.current, submittedSession));
      removeRuntimeAbort();
      if (!stale && (!controller || submissionAbortControllerRef.current === controller)) {
        submissionAbortControllerRef.current = null;
        setIsLoading(false);
        if (!resumedAfterBusyFailure) setLoadingStartedAt(null);
      }
    }
  }, [agent, cancelResume, clearSession, client, commands, currentSessionRef, disabled, enabledGoalCommand, ensureSession, imageAttachmentsRef, isSessionLive, isStoppingRef, markAutoScrollPinned, messagesRef, namespace, onSubmitMessage, refreshNewestSessionPage, resolvedHistoryPageSize, sessionId, setError, setImageAttachments, setInput, setIsLoading, setIsResuming, setLoadingNow, setLoadingStartedAt, setMessages, setStreamEvents, startResume, submissionAbortControllerRef, submittedPreviewUrlsRef, uploadQueuedImages, waitForCanonicalAssistantUpdate]);

  return { submitMessage };
}
