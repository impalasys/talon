import { useCallback, useEffect, useRef, useState } from "react";
import type { Dispatch, SetStateAction } from "react";

export type SessionAttachment = {
  id: string;
  file: File;
  previewUrl: string;
  status: "queued" | "uploading" | "ready" | "error";
  object?: unknown;
  error?: string;
};

export function useSessionAttachments<T extends SessionAttachment = SessionAttachment>(initial: T[] = []) {
  const [attachments, setAttachments] = useState<T[]>(initial);
  const attachmentsRef = useRef(attachments);
  attachmentsRef.current = attachments;

  const remove = useCallback((id: string) => {
    setAttachments((current) => {
      const removed = current.find((item) => item.id === id);
      if (removed) URL.revokeObjectURL(removed.previewUrl);
      return current.filter((item) => item.id !== id);
    });
  }, []);

  const replace = useCallback<Dispatch<SetStateAction<T[]>>>((next) => {
    setAttachments(next);
  }, []);

  useEffect(() => () => {
    for (const attachment of attachmentsRef.current) URL.revokeObjectURL(attachment.previewUrl);
  }, []);

  return { attachments, attachmentsRef, replace, remove };
}
