import { useCallback, useMemo } from "react";
import type {
  TalonImageUploadContext,
  TalonImageUploadResult,
  TalonSessionPendingImageAttachment,
} from "../TalonSession";
import { normalizeImageUploadResult } from "./objectRefs";
import { useSessionAttachments } from "./hooks/useSessionAttachments";

export type UseSessionImageAttachmentsOptions = {
  acceptedImageTypes: string[];
  createId: () => string;
  maxImageAttachments: number;
  maxImageBytes: number;
  onError: (error: Error | null) => void;
  onUpload?: (context: TalonImageUploadContext) => Promise<TalonImageUploadResult>;
};

export function useSessionImageAttachments({
  acceptedImageTypes, createId, maxImageAttachments, maxImageBytes, onError, onUpload,
}: UseSessionImageAttachmentsOptions) {
  const { attachments, attachmentsRef, replace } = useSessionAttachments<TalonSessionPendingImageAttachment>();
  const acceptedTypes = useMemo(() => new Set(acceptedImageTypes), [acceptedImageTypes]);

  const remove = useCallback((id: string) => {
    replace((current) => {
      const attachment = current.find((item) => item.id === id);
      if (attachment) URL.revokeObjectURL(attachment.previewUrl);
      return current.filter((item) => item.id !== id);
    });
  }, [replace]);

  const addFiles = useCallback((files: File[]) => {
    if (!onUpload) return;
    onError(null);
    replace((current) => {
      const availableSlots = Math.max(0, maxImageAttachments - current.length);
      const next = [...current];
      for (const file of files.slice(0, availableSlots)) {
        if (!acceptedTypes.has(file.type)) {
          next.push({ id: createId(), file, previewUrl: URL.createObjectURL(file), status: "error", error: `Unsupported image type: ${file.type || "unknown"}` });
        } else if (file.size > maxImageBytes) {
          next.push({ id: createId(), file, previewUrl: URL.createObjectURL(file), status: "error", error: `Image is larger than ${Math.round(maxImageBytes / (1024 * 1024))} MB` });
        } else {
          next.push({ id: createId(), file, previewUrl: URL.createObjectURL(file), status: "queued" });
        }
      }
      if (files.length > availableSlots) onError(new Error(`You can attach up to ${maxImageAttachments} images.`));
      return next;
    });
  }, [acceptedTypes, createId, maxImageAttachments, maxImageBytes, onError, onUpload, replace]);

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
      object: normalizeImageUploadResult(await onUpload({
        file: attachment.file,
        namespace: session.ns,
        agent: session.agent,
        sessionId: session.sessionId,
        signal,
      })),
    })));
    const results = new Map<string, { error?: string; object?: TalonSessionPendingImageAttachment["object"] }>();
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
