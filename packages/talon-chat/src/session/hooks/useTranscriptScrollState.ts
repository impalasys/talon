import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { CopilotMessage } from "../../lib/chatTimeline";

const AUTO_SCROLL_BOTTOM_THRESHOLD_PX = 48;
const useSafeLayoutEffect = typeof window !== "undefined" ? useLayoutEffect : useEffect;

export type SessionScrollThumb = { visible: boolean; top: number; height: number };

type Options = {
  messages: CopilotMessage[];
  sessionKey: string | null;
  isLive: boolean;
  error: Error | null;
  streamEvents: unknown[];
  hydrationState: Record<string, unknown>;
  expandedThinkingMessages: Record<string, boolean>;
  expandedToolItems: Record<string, boolean>;
};

function isNearScrollBottom(container: HTMLElement) {
  return container.scrollHeight - container.scrollTop - container.clientHeight <= AUTO_SCROLL_BOTTOM_THRESHOLD_PX;
}

/** Owns transcript DOM refs, automatic scrolling, and the visual scroll thumb. */
export function useTranscriptScrollState({
  messages, sessionKey, isLive, error, streamEvents, hydrationState, expandedThinkingMessages, expandedToolItems,
}: Options) {
  const [scrollThumb, setScrollThumb] = useState<SessionScrollThumb>({ visible: false, top: 0, height: 0 });
  const transcriptRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const skipNextAutoScrollRef = useRef(false);
  const autoScrollPinnedRef = useRef(true);

  const updateScrollThumb = useCallback(() => {
    const container = transcriptRef.current;
    if (!container) return;
    if (container.scrollHeight <= container.clientHeight + 1) {
      setScrollThumb((current) => current.visible ? { visible: false, top: 0, height: 0 } : current);
      return;
    }
    const inset = 8;
    const trackHeight = Math.max(0, container.clientHeight - inset * 2);
    const height = Math.max(32, Math.round((container.clientHeight / container.scrollHeight) * trackHeight));
    const maxScrollTop = container.scrollHeight - container.clientHeight;
    const maxTravel = Math.max(0, trackHeight - height);
    const top = Math.round(inset + maxTravel * (maxScrollTop > 0 ? container.scrollTop / maxScrollTop : 0));
    const next = { visible: true, top, height };
    setScrollThumb((current) =>
      current.visible === next.visible && current.top === next.top && current.height === next.height ? current : next,
    );
  }, []);

  const scrollToBottom = useCallback((behavior: ScrollBehavior) => {
    autoScrollPinnedRef.current = true;
    const container = transcriptRef.current;
    if (container && typeof container.scrollTo === "function") {
      container.scrollTo({ top: container.scrollHeight, behavior });
      return;
    }
    if (container) container.scrollTop = container.scrollHeight;
    bottomRef.current?.scrollIntoView({ behavior });
  }, []);

  useEffect(() => {
    if (skipNextAutoScrollRef.current) {
      skipNextAutoScrollRef.current = false;
      return;
    }
    const rafId = window.requestAnimationFrame(() => {
      if (autoScrollPinnedRef.current) scrollToBottom("auto");
      updateScrollThumb();
    });
    return () => window.cancelAnimationFrame(rafId);
  }, [sessionKey, messages, streamEvents, isLive, error, scrollToBottom, updateScrollThumb]);

  useEffect(() => {
    updateScrollThumb();
    window.addEventListener("resize", updateScrollThumb);
    return () => window.removeEventListener("resize", updateScrollThumb);
  }, [updateScrollThumb]);

  useSafeLayoutEffect(() => {
    updateScrollThumb();
  }, [messages, expandedThinkingMessages, expandedToolItems, hydrationState, isLive, error, streamEvents, updateScrollThumb]);

  const handleScroll = useCallback(() => {
    updateScrollThumb();
    const container = transcriptRef.current;
    if (container) autoScrollPinnedRef.current = isNearScrollBottom(container);
  }, [updateScrollThumb]);

  const reset = useCallback(() => {
    autoScrollPinnedRef.current = true;
    skipNextAutoScrollRef.current = false;
  }, []);

  return {
    bottomRef,
    handleScroll,
    markAutoScrollPinned: () => { autoScrollPinnedRef.current = true; },
    reset,
    scrollThumb,
    skipNextAutoScroll: () => { skipNextAutoScrollRef.current = true; },
    transcriptRef,
    updateScrollThumb,
  };
}
