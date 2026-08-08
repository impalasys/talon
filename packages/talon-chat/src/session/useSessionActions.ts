import { useCallback } from "react";
import type { SessionActionsOptions } from "./sessionActionTypes";
import { useCanonicalAssistantRefresh } from "./useCanonicalAssistantRefresh";
import { prepareSubmission, useSessionSubmissionRouting } from "./useSessionSubmissionRouting";
import { useSessionTargetResolution } from "./useSessionTargetResolution";
import { useSessionTurnSubmitter } from "./useSessionTurnSubmitter";

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
export function useSessionActions(options: SessionActionsOptions) {
  const ensureSession = useSessionTargetResolution(options);
  const waitForCanonicalAssistantUpdate = useCanonicalAssistantRefresh(options);
  const { runHostSubmission, runSessionCommand } = useSessionSubmissionRouting({ ...options, ensureSession });
  const submitSessionTurnAndRecover = useSessionTurnSubmitter({
    ...options,
    createMessageId: createLocalMessageId,
    ensureSession,
    waitForCanonicalAssistantUpdate,
  });

  const submitMessage = useCallback(async (submittedText: string, invokedByRuntime = false, runtimeSignal?: AbortSignal) => {
    const initialText = submittedText.trim();
    const pendingAttachments = options.imageAttachmentsRef.current;
    const hasImages = pendingAttachments.length > 0;
    if ((!initialText && !hasImages) || (!invokedByRuntime && options.isSessionLive) || options.disabled) return;
    if (await runHostSubmission(initialText, pendingAttachments)) return;
    const prepared = prepareSubmission(initialText, options.enabledGoalCommand);
    if (prepared.error) {
      options.setError(prepared.error);
      return;
    }
    if (await runSessionCommand(prepared.text, prepared.parsedCommand, hasImages)) return;
    await submitSessionTurnAndRecover(prepared.text, pendingAttachments, submittedText, runtimeSignal);
  }, [options, runHostSubmission, runSessionCommand, submitSessionTurnAndRecover]);

  return { submitMessage };
}
