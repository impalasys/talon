import { useEffect, useMemo, useRef, useState } from "react";
import type { MutableRefObject } from "react";
import type { CopilotMessage } from "../lib/chatTimeline";
import type { SessionTarget } from "./types";

type UseSessionPresentationStateOptions = {
  abortControllerRef: MutableRefObject<AbortController | null>;
  currentSession: SessionTarget | null;
  input: string;
  isSessionLive: boolean;
  loadingStartedAt: string | number | null;
  messages: CopilotMessage[];
};

/** Owns refs and lightweight display state that are shared by session actions and the transcript. */
export function useSessionPresentationState({
  abortControllerRef,
  currentSession,
  input,
  isSessionLive,
  loadingStartedAt,
  messages,
}: UseSessionPresentationStateOptions) {
  const messagesRef = useRef<CopilotMessage[]>(messages);
  const currentSessionRef = useRef<SessionTarget | null>(currentSession);
  const submittedPreviewUrlsRef = useRef<string[]>([]);
  const [loadingNow, setLoadingNow] = useState(Date.now());
  const inputRows = useMemo(() => Math.min(input.split("\n").length, 8), [input]);

  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);
  useEffect(() => {
    currentSessionRef.current = currentSession;
  }, [currentSession]);
  useEffect(() => {
    if (!isSessionLive || loadingStartedAt === null) return;
    setLoadingNow(Date.now());
    const intervalId = window.setInterval(() => setLoadingNow(Date.now()), 250);
    return () => window.clearInterval(intervalId);
  }, [isSessionLive, loadingStartedAt]);
  useEffect(() => () => {
    abortControllerRef.current?.abort();
    for (const previewUrl of submittedPreviewUrlsRef.current) URL.revokeObjectURL(previewUrl);
  }, [abortControllerRef]);

  return { currentSessionRef, inputRows, loadingNow, messagesRef, setLoadingNow, submittedPreviewUrlsRef };
}
