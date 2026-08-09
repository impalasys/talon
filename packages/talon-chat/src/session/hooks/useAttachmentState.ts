import { useCallback, useEffect, useRef, useState } from "react";
import type { Dispatch, SetStateAction } from "react";

export type SessionAttachmentState = {
  id: string;
  file: File;
  /** Present only when the attachment can be previewed inline. */
  previewUrl?: string;
  status: "queued" | "uploading" | "ready" | "error";
  object?: unknown;
  error?: string;
};

/** Owns attachment state and releases any object URLs it creates. */
export function useAttachmentState<T extends SessionAttachmentState = SessionAttachmentState>(initial: T[] = []) {
  const [attachments, setAttachments] = useState<T[]>(initial);
  const attachmentsRef = useRef(attachments);
  attachmentsRef.current = attachments;

  const replace = useCallback<Dispatch<SetStateAction<T[]>>>((next) => {
    setAttachments(next);
  }, []);

  useEffect(() => () => {
    for (const attachment of attachmentsRef.current) {
      if (attachment.previewUrl) URL.revokeObjectURL(attachment.previewUrl);
    }
  }, []);

  return { attachments, attachmentsRef, replace };
}
