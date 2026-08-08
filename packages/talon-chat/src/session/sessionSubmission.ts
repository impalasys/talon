import { data, type TalonClient } from "@impalasys/talon-client";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import { getMessageContent, type CopilotMessage } from "../lib/chatTimeline";
import { type StreamEventItem, streamSessionPartEvents } from "../lib/uiStream";
import { normalizeObjectRefForJson, objectRefMediaType } from "./objectRefs";
import { protoSessionPartsFromChatParts } from "./protocol";
import type { TalonSessionPendingImageAttachment } from "./TalonSessionTypes";
import type { SessionTarget } from "./types";

export type SessionActionsClient = Pick<TalonClient["sessions"], "submitTurn">;
export type RefreshSession = (target: SessionTarget, signal?: AbortSignal) => Promise<{ state?: string; messages: CopilotMessage[] } | null>;

export function sameSession(left: SessionTarget | null, right: SessionTarget | null) {
  return left?.ns === right?.ns && left?.agent === right?.agent && left?.sessionId === right?.sessionId;
}

export function isBusySessionError(error: unknown) {
  const candidate = error as { message?: unknown } | null;
  return typeof candidate?.message === "string" && /session is currently generating|session is busy/i.test(candidate.message);
}

export function normalizeEpochToMilliseconds(value: unknown) {
  const numericValue = typeof value === "string" ? Number(value) : value;
  const normalized = typeof numericValue === "number" && Number.isFinite(numericValue) ? numericValue : null;
  if (!normalized || normalized <= 0) return null;
  if (normalized >= 1e15) return Math.trunc(normalized / 1000);
  if (normalized >= 1e12) return Math.trunc(normalized);
  if (normalized >= 1e9) return Math.trunc(normalized * 1000);
  return null;
}

export function assistantSignature(messages: CopilotMessage[]) {
  return messages
    .filter((message) => message.role === "assistant")
    .map((message) => `${message.id}:${getMessageContent(message).length}`)
    .join("|");
}

function imageParts(attachments: TalonSessionPendingImageAttachment[]) {
  return attachments.map((attachment) => {
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
}

type SubmitSessionTurnOptions = {
  client: SessionActionsClient | undefined;
  controller: AbortController;
  createMessageId: () => string;
  markAutoScrollPinned: () => void;
  onOptimisticMessage: (id: string) => void;
  onTurnStarted: () => void;
  pendingAttachments: TalonSessionPendingImageAttachment[];
  session: SessionTarget;
  setImageAttachments: Dispatch<SetStateAction<TalonSessionPendingImageAttachment[]>>;
  setInput: Dispatch<SetStateAction<string>>;
  setIsLoading: (value: boolean) => void;
  setLoadingNow: (value: number) => void;
  setLoadingStartedAt: (value: string | number | null) => void;
  setMessages: Dispatch<SetStateAction<CopilotMessage[]>>;
  setStreamEvents: Dispatch<SetStateAction<StreamEventItem[]>>;
  submittedPreviewUrlsRef: MutableRefObject<string[]>;
  text: string;
  uploadQueuedImages: (target: SessionTarget, signal: AbortSignal) => Promise<TalonSessionPendingImageAttachment[]>;
};

/** Uploads attachments, creates the optimistic user row, and streams exactly one backend turn. */
export async function submitSessionTurn({
  client,
  controller,
  createMessageId,
  markAutoScrollPinned,
  onOptimisticMessage,
  onTurnStarted,
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
}: SubmitSessionTurnOptions) {
  const uploadedImages = await uploadQueuedImages(session, controller.signal);
  const userMessage: CopilotMessage = {
    id: createMessageId(),
    role: "user",
    content: text,
    parts: [...(text ? [{ type: "text", text }] : []), ...imageParts(uploadedImages)],
    createdAt: String(Date.now() * 1000),
  };
  onOptimisticMessage(userMessage.id);
  setInput("");
  submittedPreviewUrlsRef.current.push(...uploadedImages.map((attachment) => attachment.previewUrl));
  setImageAttachments([]);
  setMessages((previous) => [...previous, userMessage]);
  setLoadingStartedAt(normalizeEpochToMilliseconds(userMessage.createdAt) ?? Date.now());
  setLoadingNow(Date.now());
  markAutoScrollPinned();
  setIsLoading(true);
  if (!client?.submitTurn) throw new Error("TalonSession requires a Talon clientset with sessions.submitTurn().");
  onTurnStarted();
  return streamSessionPartEvents({
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
}

type RecoverBusySessionTurnOptions = {
  controller: AbortController | null;
  error: Error;
  imageAttachmentsRef: MutableRefObject<TalonSessionPendingImageAttachment[]>;
  isStoppingRef: MutableRefObject<boolean>;
  messagesRef: MutableRefObject<CopilotMessage[]>;
  optimisticMessageId: string | null;
  pendingAttachments: TalonSessionPendingImageAttachment[];
  refreshNewestSessionPage: RefreshSession;
  session: SessionTarget | null;
  setError: (error: Error | null) => void;
  setImageAttachments: Dispatch<SetStateAction<TalonSessionPendingImageAttachment[]>>;
  setInput: Dispatch<SetStateAction<string>>;
  setMessages: Dispatch<SetStateAction<CopilotMessage[]>>;
  startResume: (target: SessionTarget) => void;
  submittedText: string;
  currentSessionRef: MutableRefObject<SessionTarget | null>;
};

export type BusyTurnRecovery = "not-recovered" | "discarded" | "resumed";

function restoreOptimisticMessage({
  imageAttachmentsRef,
  messagesRef,
  optimisticMessageId,
  pendingAttachments,
  setImageAttachments,
  setInput,
  setMessages,
  submittedText,
}: Pick<RecoverBusySessionTurnOptions,
  "imageAttachmentsRef" | "messagesRef" | "optimisticMessageId" | "pendingAttachments" |
  "setImageAttachments" | "setInput" | "setMessages" | "submittedText"
>) {
  if (!optimisticMessageId) return;
  messagesRef.current = messagesRef.current.filter((message) => message.id !== optimisticMessageId);
  setMessages((previous) => previous.filter((message) => message.id !== optimisticMessageId));
  setInput((current) => current || submittedText.trim());
  if (imageAttachmentsRef.current.length !== 0 || pendingAttachments.length === 0) return;
  imageAttachmentsRef.current = pendingAttachments;
  setImageAttachments(pendingAttachments);
}

/** Restores the local composer and reconnects when a concurrent server turn owns the session. */
export async function recoverBusySessionTurn({
  controller,
  error,
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
  currentSessionRef,
}: RecoverBusySessionTurnOptions): Promise<BusyTurnRecovery> {
  if (!session || !isBusySessionError(error)) return "not-recovered";
  restoreOptimisticMessage({ imageAttachmentsRef, messagesRef, optimisticMessageId, pendingAttachments, setImageAttachments, setInput, setMessages, submittedText });
  const refreshed = await refreshNewestSessionPage(session, controller?.signal).catch(() => null);
  if (controller?.signal.aborted || !sameSession(currentSessionRef.current, session) || isStoppingRef.current) return "discarded";
  if (refreshed?.state !== "PROCESSING") return "not-recovered";
  setError(null);
  startResume(session);
  return "resumed";
}

export function createTurnController(runtimeSignal?: AbortSignal) {
  const controller = new AbortController();
  const abortFromRuntime = () => controller.abort();
  if (runtimeSignal?.aborted) controller.abort();
  else runtimeSignal?.addEventListener("abort", abortFromRuntime, { once: true });
  return { controller, removeRuntimeAbort: () => runtimeSignal?.removeEventListener("abort", abortFromRuntime) };
}

type RecoverFailedSessionTurnOptions = Omit<RecoverBusySessionTurnOptions, "error"> & {
  error: Error;
  resolvedHistoryPageSize: number;
  turnStarted: boolean;
  waitForCanonicalAssistantUpdate: (session: SessionTarget, signature: string, signal?: AbortSignal) => Promise<boolean>;
};

/** Handles expected concurrent-turn recovery and delayed canonical history after a stream failure. */
export async function recoverFailedSessionTurn({
  controller,
  currentSessionRef,
  error,
  resolvedHistoryPageSize,
  session,
  turnStarted,
  waitForCanonicalAssistantUpdate,
  ...busyRecoveryOptions
}: RecoverFailedSessionTurnOptions) {
  if (controller?.signal.aborted || (session && !sameSession(currentSessionRef.current, session))) return "handled" as const;
  const busyRecovery = await recoverBusySessionTurn({
    controller,
    currentSessionRef,
    error,
    session,
    ...busyRecoveryOptions,
  });
  if (busyRecovery !== "not-recovered") return busyRecovery === "resumed" ? "resumed" as const : "handled" as const;
  if (!session || !turnStarted || isBusySessionError(error)) return "unhandled" as const;
  const baselineSignature = assistantSignature(busyRecoveryOptions.messagesRef.current.slice(-resolvedHistoryPageSize));
  const refreshed = await waitForCanonicalAssistantUpdate(session, baselineSignature, controller?.signal).catch(() => false);
  if (!refreshed) return "unhandled" as const;
  busyRecoveryOptions.setError(null);
  return "handled" as const;
}

type FinishSessionTurnOptions = {
  controller: AbortController | null;
  currentSessionRef: MutableRefObject<SessionTarget | null>;
  resumedAfterBusyFailure: boolean;
  session: SessionTarget | null;
  setIsLoading: (value: boolean) => void;
  setLoadingStartedAt: (value: string | number | null) => void;
  submissionAbortControllerRef: MutableRefObject<AbortController | null>;
};

export function finishSessionTurn({
  controller,
  currentSessionRef,
  resumedAfterBusyFailure,
  session,
  setIsLoading,
  setLoadingStartedAt,
  submissionAbortControllerRef,
}: FinishSessionTurnOptions) {
  const stale = controller?.signal.aborted || (session && !sameSession(currentSessionRef.current, session));
  if (stale || (controller && submissionAbortControllerRef.current !== controller)) return;
  submissionAbortControllerRef.current = null;
  setIsLoading(false);
  if (!resumedAfterBusyFailure) setLoadingStartedAt(null);
}
