import { useCallback } from "react";
import type { TalonSessionPendingImageAttachment } from "./TalonSessionTypes";
import type { SessionActionsOptions } from "./sessionActionTypes";
import {
  assistantSignature,
  createTurnController,
  finishSessionTurn,
  recoverFailedSessionTurn,
  sameSession,
  submitSessionTurn,
} from "./sessionSubmission";
import type { SessionTarget } from "./types";

type UseSessionTurnSubmitterOptions = Pick<SessionActionsOptions,
  "cancelResume" | "client" | "currentSessionRef" | "imageAttachmentsRef" | "isStoppingRef" |
  "markAutoScrollPinned" | "messagesRef" | "refreshNewestSessionPage" | "resolvedHistoryPageSize" |
  "setError" | "setImageAttachments" | "setInput" | "setIsLoading" | "setIsResuming" | "setLoadingNow" |
  "setLoadingStartedAt" | "setMessages" | "setStreamEvents" | "startResume" | "submissionAbortControllerRef" |
  "submittedPreviewUrlsRef" | "uploadQueuedImages"
> & {
  createMessageId: () => string;
  ensureSession: () => Promise<SessionTarget>;
  waitForCanonicalAssistantUpdate: (session: SessionTarget, signature: string, signal?: AbortSignal) => Promise<boolean>;
};

/** Starts one backend turn and keeps retry, abort, and canonical-history cleanup in one boundary. */
export function useSessionTurnSubmitter({
  cancelResume,
  client,
  createMessageId,
  currentSessionRef,
  ensureSession,
  imageAttachmentsRef,
  isStoppingRef,
  markAutoScrollPinned,
  messagesRef,
  refreshNewestSessionPage,
  resolvedHistoryPageSize,
  setError,
  setImageAttachments,
  setInput,
  setIsLoading,
  setIsResuming,
  setLoadingNow,
  setLoadingStartedAt,
  setMessages,
  setStreamEvents,
  startResume,
  submissionAbortControllerRef,
  submittedPreviewUrlsRef,
  uploadQueuedImages,
  waitForCanonicalAssistantUpdate,
}: UseSessionTurnSubmitterOptions) {
  return useCallback(async (
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
        createMessageId,
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
    } catch (error) {
      const nextError = error instanceof Error ? error : new Error(String(error));
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
  }, [cancelResume, client, createMessageId, currentSessionRef, ensureSession, imageAttachmentsRef, isStoppingRef, markAutoScrollPinned, messagesRef, refreshNewestSessionPage, resolvedHistoryPageSize, setError, setImageAttachments, setInput, setIsLoading, setIsResuming, setLoadingNow, setLoadingStartedAt, setMessages, setStreamEvents, startResume, submissionAbortControllerRef, submittedPreviewUrlsRef, uploadQueuedImages, waitForCanonicalAssistantUpdate]);
}
