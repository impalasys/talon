import { useCallback, useMemo } from "react";
import type {
  TalonAttachmentUploadContext,
  TalonAttachmentUploadResult,
  TalonSessionPendingAttachment,
} from "../../TalonSession";
import { normalizeAttachmentUploadResult } from "../objectRefs";
import { useAttachmentState } from "./useAttachmentState";

export type UseSessionAttachmentsOptions = {
  acceptedTypes: string[];
  createId: () => string;
  maxAttachments: number;
  maxBytes: number;
  onError: (error: Error | null) => void;
  onUpload?: (context: TalonAttachmentUploadContext) => Promise<TalonAttachmentUploadResult>;
};

/** Validates, uploads, and tracks arbitrary composer attachments. */
export function useSessionAttachments({
  acceptedTypes: acceptedTypeValues,
  createId,
  maxAttachments,
  maxBytes,
  onError,
  onUpload,
}: UseSessionAttachmentsOptions) {
  const { attachments, attachmentsRef, replace } = useAttachmentState<TalonSessionPendingAttachment>();
  const acceptedTypes = useMemo(() => new Set(acceptedTypeValues), [acceptedTypeValues]);

  const remove = useCallback((id: string) => {
    replace((current) => {
      const attachment = current.find((item) => item.id === id);
      if (attachment?.previewUrl) URL.revokeObjectURL(attachment.previewUrl);
      return current.filter((item) => item.id !== id);
    });
  }, [replace]);

  const addFiles = useCallback((files: File[]) => {
    if (!onUpload) return;
    onError(null);
    replace((current) => {
      const availableSlots = Math.max(0, maxAttachments - current.length);
      const next = [...current];
      for (const file of files.slice(0, availableSlots)) {
        const previewUrl = file.type.startsWith("image/") ? URL.createObjectURL(file) : undefined;
        if (!acceptedTypes.has(file.type)) {
          next.push({ id: createId(), file, previewUrl, status: "error", error: `Unsupported attachment type: ${file.type || "unknown"}` });
        } else if (file.size > maxBytes) {
          next.push({ id: createId(), file, previewUrl, status: "error", error: `Attachment is larger than ${Math.round(maxBytes / (1024 * 1024))} MB` });
        } else {
          next.push({ id: createId(), file, previewUrl, status: "queued" });
        }
      }
      if (files.length > availableSlots) onError(new Error(`You can attach up to ${maxAttachments} files.`));
      return next;
    });
  }, [acceptedTypes, createId, maxAttachments, maxBytes, onError, onUpload, replace]);

  const uploadQueued = useCallback(async (
    session: { ns: string; agent: string; sessionId: string },
    signal: AbortSignal,
  ) => {
    if (!onUpload) return attachmentsRef.current;
    const existing = attachmentsRef.current;
    const failed = existing.find((attachment) => attachment.status === "error");
    if (failed) throw new Error(failed.error || `Failed to attach ${failed.file.name}`);
    const pending = existing.filter((attachment) => !attachment.object);
    if (pending.length === 0) return existing;

    const pendingIds = new Set(pending.map((attachment) => attachment.id));
    const uploading = attachmentsRef.current.map((attachment) =>
      pendingIds.has(attachment.id) ? { ...attachment, status: "uploading" as const, error: undefined } : attachment,
    );
    attachmentsRef.current = uploading;
    replace(uploading);

    const settled = await Promise.allSettled(pending.map(async (attachment) => ({
      id: attachment.id,
      object: normalizeAttachmentUploadResult(await onUpload({
        file: attachment.file,
        namespace: session.ns,
        agent: session.agent,
        sessionId: session.sessionId,
        signal,
      })),
    })));
    const results = new Map<string, { error?: string; object?: TalonSessionPendingAttachment["object"] }>();
    settled.forEach((result, index) => {
      const attachment = pending[index];
      if (!attachment) return;
      results.set(attachment.id, result.status === "fulfilled"
        ? { object: result.value.object }
        : { error: result.reason instanceof Error ? result.reason.message : String(result.reason || `Failed to attach ${attachment.file.name}`) });
    });
    const next = attachmentsRef.current.map((attachment) => {
      const result = results.get(attachment.id);
      if (!result) return attachment;
      return result.object
        ? { ...attachment, object: result.object, status: "ready" as const, error: undefined }
        : { ...attachment, status: "error" as const, error: result.error || `Failed to attach ${attachment.file.name}` };
    });
    attachmentsRef.current = next;
    replace(next);
    const uploadFailure = next.find((attachment) => attachment.status === "error");
    if (uploadFailure) throw new Error(uploadFailure.error || `Failed to attach ${uploadFailure.file.name}`);
    return next;
  }, [attachmentsRef, onUpload, replace]);

  return { addFiles, attachments, attachmentsRef, remove, replace, uploadQueued };
}
