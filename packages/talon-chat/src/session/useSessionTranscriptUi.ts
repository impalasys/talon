import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { CopilotMessage } from "../lib/chatTimeline";

const HISTORY_SCROLL_LOAD_THRESHOLD_PX = 120;
const AUTO_SCROLL_BOTTOM_THRESHOLD_PX = 48;
const useSafeLayoutEffect = typeof window !== "undefined" ? useLayoutEffect : useEffect;

type ScrollRestore = { previousScrollTop: number; previousScrollHeight: number };

export type SessionScrollThumb = { visible: boolean; top: number; height: number };

export type UseSessionTranscriptUiOptions = {
  messages: CopilotMessage[];
  sessionKey: string | null;
  isLive: boolean;
  error: Error | null;
  streamEvents: unknown[];
  hydrationState: Record<string, unknown>;
  canLoadOlder: boolean;
  onLoadOlder: () => Promise<boolean>;
};

function isNearScrollBottom(container: HTMLElement) {
  return container.scrollHeight - container.scrollTop - container.clientHeight <= AUTO_SCROLL_BOTTOM_THRESHOLD_PX;
}

export function useSessionTranscriptUi({
  messages, sessionKey, isLive, error, streamEvents, hydrationState, canLoadOlder, onLoadOlder,
}: UseSessionTranscriptUiOptions) {
  const [expandedThinkingMessages, setExpandedThinkingMessages] = useState<Record<string, boolean>>({});
  const [expandedToolItems, setExpandedToolItems] = useState<Record<string, boolean>>({});
  const [scrollThumb, setScrollThumb] = useState<SessionScrollThumb>({ visible: false, top: 0, height: 0 });
  const transcriptRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const skipNextAutoScrollRef = useRef(false);
  const autoScrollPinnedRef = useRef(true);
  const prependScrollRestoreRef = useRef<ScrollRestore | null>(null);
  const isLoadingOlderHistoryRef = useRef(false);

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

  useSafeLayoutEffect(() => {
    const restore = prependScrollRestoreRef.current;
    const container = transcriptRef.current;
    if (!restore || !container) return;
    container.scrollTop = restore.previousScrollTop + container.scrollHeight - restore.previousScrollHeight;
    prependScrollRestoreRef.current = null;
    updateScrollThumb();
  }, [messages, updateScrollThumb]);

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

  const toggleThinkingMessage = useCallback((messageId: string) => {
    setExpandedThinkingMessages((current) => ({ ...current, [messageId]: !current[messageId] }));
  }, []);
  const toggleToolItem = useCallback((toolKey: string) => {
    setExpandedToolItems((current) => ({ ...current, [toolKey]: !current[toolKey] }));
  }, []);

  const reset = useCallback(() => {
    setExpandedThinkingMessages({});
    setExpandedToolItems({});
    autoScrollPinnedRef.current = true;
    prependScrollRestoreRef.current = null;
  }, []);

  const handleScroll = useCallback(() => {
    updateScrollThumb();
    const container = transcriptRef.current;
    if (!container) return;
    if (!prependScrollRestoreRef.current) autoScrollPinnedRef.current = isNearScrollBottom(container);
    if (container.scrollTop > HISTORY_SCROLL_LOAD_THRESHOLD_PX || !canLoadOlder || isLoadingOlderHistoryRef.current) return;
    prependScrollRestoreRef.current = { previousScrollTop: container.scrollTop, previousScrollHeight: container.scrollHeight };
    skipNextAutoScrollRef.current = true;
    isLoadingOlderHistoryRef.current = true;
    void onLoadOlder().then((loaded) => {
      if (!loaded) {
        prependScrollRestoreRef.current = null;
        skipNextAutoScrollRef.current = false;
      }
    }).catch((error) => {
      prependScrollRestoreRef.current = null;
      skipNextAutoScrollRef.current = false;
      console.warn("Could not load older session history", error);
    }).finally(() => {
      isLoadingOlderHistoryRef.current = false;
    });
  }, [canLoadOlder, onLoadOlder, updateScrollThumb]);

  return {
    bottomRef,
    expandedThinkingMessages,
    expandedToolItems,
    handleScroll,
    markAutoScrollPinned: () => { autoScrollPinnedRef.current = true; },
    reset,
    scrollThumb,
    toggleThinkingMessage,
    toggleToolItem,
    transcriptRef,
  };
}
