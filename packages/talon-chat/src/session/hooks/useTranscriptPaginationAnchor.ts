import { useCallback, useEffect, useLayoutEffect, useRef } from "react";
import type React from "react";
import type { CopilotMessage } from "../../lib/chatTimeline";

const HISTORY_SCROLL_LOAD_THRESHOLD_PX = 120;
const useSafeLayoutEffect = typeof window !== "undefined" ? useLayoutEffect : useEffect;

type ScrollRestore = { previousScrollTop: number; previousScrollHeight: number };

type Options = {
  messages: CopilotMessage[];
  transcriptRef: React.RefObject<HTMLDivElement | null>;
  canLoadOlder: boolean;
  onLoadOlder: () => Promise<boolean>;
  onPrependStart: () => void;
  onRestored: () => void;
};

/** Preserves the visible transcript anchor while older history is prepended. */
export function useTranscriptPaginationAnchor({
  messages, transcriptRef, canLoadOlder, onLoadOlder, onPrependStart, onRestored,
}: Options) {
  const restoreRef = useRef<ScrollRestore | null>(null);
  const isLoadingRef = useRef(false);

  useSafeLayoutEffect(() => {
    const restore = restoreRef.current;
    const container = transcriptRef.current;
    if (!restore || !container) return;
    container.scrollTop = restore.previousScrollTop + container.scrollHeight - restore.previousScrollHeight;
    restoreRef.current = null;
    onRestored();
  }, [messages, onRestored, transcriptRef]);

  const handleScroll = useCallback(() => {
    const container = transcriptRef.current;
    if (!container || container.scrollTop > HISTORY_SCROLL_LOAD_THRESHOLD_PX || !canLoadOlder || isLoadingRef.current) return;
    restoreRef.current = { previousScrollTop: container.scrollTop, previousScrollHeight: container.scrollHeight };
    onPrependStart();
    isLoadingRef.current = true;
    void onLoadOlder().then((loaded) => {
      if (!loaded) restoreRef.current = null;
    }).catch((error) => {
      restoreRef.current = null;
      console.warn("Could not load older session history", error);
    }).finally(() => {
      isLoadingRef.current = false;
    });
  }, [canLoadOlder, onLoadOlder, onPrependStart, transcriptRef]);

  const reset = useCallback(() => {
    restoreRef.current = null;
    isLoadingRef.current = false;
  }, []);

  return { handleScroll, reset };
}
