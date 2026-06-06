"use client";

import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Activity, ChevronRight, Wrench } from "lucide-react";
import {
  formatUsageSummary,
  getMessageAssistantTimeline,
  getMessageContent,
  getMessageReasoningContent,
  getMessageUsage,
  hydrateMessagesWithSteps,
  normalizeMessageRole,
  type AssistantTimelineItem,
  type CopilotMessage,
} from "./lib/chatTimeline";
import { ChatInputBox } from "./lib/ChatInputBox";
import { buildGatewayHeaders, normalizeGatewayUrl } from "./lib/grpc";
import { MarkdownMessage } from "./lib/MarkdownMessage";
import { streamSessionResume, streamUiSubmission, type StreamEventItem } from "./lib/uiStream";

const useSafeLayoutEffect = typeof window !== "undefined" ? useLayoutEffect : useEffect;

export type GatewayClientLike = {
  createSession(request: { ns: string; agent: string }): Promise<{ sessionId: string }>;
  listSessionMessages?(request: {
    ns: string;
    agent: string;
    sessionId: string;
    pageSize: number;
    beforeMessageId?: string;
  }): Promise<any>;
  getSession(request: { ns: string; agent: string; sessionId: string; messageLimit?: number; stepLimit?: number }): Promise<any>;
};

export type TalonSessionProps = {
  namespace: string;
  agent: string;
  gatewayUrl: string;
  authToken?: string | null;
  gatewayClient?: GatewayClientLike;
  sessionId?: string;
  onSessionChange?: (sessionId: string) => void;
  className?: string;
  style?: React.CSSProperties;
  placeholder?: string;
  autoFocus?: boolean;
  disabled?: boolean;
  historyPageSize?: number;
  historyMessageLimit?: number;
  historyStepLimit?: number;
};

export type TalonCopilotProps = TalonSessionProps;

const emptyMessages: CopilotMessage[] = [];
const DEFAULT_HISTORY_PAGE_SIZE = 50;
const DEFAULT_HISTORY_MESSAGE_LIMIT = 100;
const DEFAULT_HISTORY_STEP_LIMIT = 1000;
const HISTORY_SCROLL_LOAD_THRESHOLD_PX = 120;

function buildGatewayChatUiUrl(gatewayUrl: string, ns: string, agent: string, sessionId: string) {
  return `${normalizeGatewayUrl(gatewayUrl)}/v1/ui/ns/${encodeURIComponent(ns)}/agents/${encodeURIComponent(agent)}/sessions/${encodeURIComponent(sessionId)}`;
}

function buildGatewaySessionMessagesUrl(
  gatewayUrl: string,
  ns: string,
  agent: string,
  sessionId: string,
  pageSize: number,
  beforeMessageId?: string,
) {
  const url = new URL(
    `${normalizeGatewayUrl(gatewayUrl)}/v1/ns/${encodeURIComponent(ns)}/agents/${encodeURIComponent(agent)}/sessions/${encodeURIComponent(sessionId)}/messages`,
  );
  url.searchParams.set("page_size", String(Math.trunc(pageSize)));
  if (beforeMessageId) {
    url.searchParams.set("before_message_id", beforeMessageId);
  }
  return url.toString();
}

function border(color: string) {
  return `1px solid ${color}`;
}

const talonChatFontFamily =
  'var(--talon-chat-font-family, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif)';

function cn(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(" ");
}

function isSameSession(
  left: { ns: string; agent: string; sessionId: string } | null,
  right: { ns: string; agent: string; sessionId: string } | null,
) {
  return (
    left?.ns === right?.ns &&
    left?.agent === right?.agent &&
    left?.sessionId === right?.sessionId
  );
}

function createLocalMessageId() {
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

function normalizeEpochToMilliseconds(value: unknown) {
  let normalized: number | null = null;
  if (typeof value === "bigint") {
    const bigintValue = value < BigInt(0) ? -value : value;
    if (bigintValue > BigInt(Number.MAX_SAFE_INTEGER)) {
      return null;
    }
    normalized = Number(value);
  } else if (typeof value === "string") {
    const numericValue = Number(value);
    normalized = Number.isFinite(numericValue) ? numericValue : Date.parse(value);
  } else if (typeof value === "number") {
    normalized = value;
  }
  if (typeof normalized !== "number" || !Number.isFinite(normalized) || normalized <= 0) {
    return null;
  }
  if (normalized >= 1e15) {
    return Math.trunc(normalized / 1000);
  }
  if (normalized >= 1e12) {
    return Math.trunc(normalized);
  }
  if (normalized >= 1e9) {
    return Math.trunc(normalized * 1000);
  }
  return null;
}

function getAssistantSignature(messages: any[] | undefined) {
  if (!Array.isArray(messages)) return "";
  return messages
    .filter((message) => message?.role === "assistant" || message?.role === 2 || message?.role === "ROLE_ASSISTANT")
    .map((message) => `${String(message.id ?? "")}:${getMessageContent(message).length}`)
    .join("|");
}

type SessionHistoryPage = {
  messages: CopilotMessage[];
  state: string;
  hasMore: boolean;
  nextBeforeMessageId: string | null;
};

type ScrollThumbState = {
  visible: boolean;
  top: number;
  height: number;
};

function stableStringHash(value: string) {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) >>> 0;
  }
  return hash.toString(36);
}

function stableHistoryMessageId(message: any, index: number) {
  if (typeof message?.id === "string" && message.id.length > 0) {
    return message.id;
  }
  const role = normalizeMessageRole(message?.role);
  const createdAt = message?.createdAt ?? message?.created_at ?? "unknown";
  const content = getMessageContent(message);
  return `history-${role}-${createdAt}-${index}-${stableStringHash(content)}`;
}

function historyMessageTimestamp(message: Pick<CopilotMessage, "createdAt">) {
  return normalizeEpochToMilliseconds(message.createdAt);
}

function formatWorkDuration(start: unknown, end: unknown) {
  const startMs = normalizeEpochToMilliseconds(start);
  const endMs = normalizeEpochToMilliseconds(end);
  if (startMs === null || endMs === null || endMs <= startMs) {
    return "Worked";
  }
  const totalSeconds = Math.max(1, Math.round((endMs - startMs) / 1000));
  if (totalSeconds < 60) {
    return `Worked for ${totalSeconds}s`;
  }
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return seconds > 0 ? `Worked for ${minutes}m ${seconds}s` : `Worked for ${minutes}m`;
}

function formatWorkingDuration(start: unknown, now = Date.now()) {
  const startMs = normalizeEpochToMilliseconds(start);
  if (startMs === null || now < startMs) {
    return "Working";
  }
  const totalSeconds = Math.max(1, Math.floor((now - startMs) / 1000));
  if (totalSeconds < 60) {
    return `Working for ${totalSeconds}s`;
  }
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return seconds > 0 ? `Working for ${minutes}m ${seconds}s` : `Working for ${minutes}m`;
}

function isLocalMessageId(id: string) {
  return id.startsWith("local-") || id.startsWith("msg-");
}

function canCompareCanonicalMessageIds(left: string, right: string) {
  const isFallbackId = (id: string) => id.startsWith("history-") || isLocalMessageId(id);
  return !isFallbackId(left) && !isFallbackId(right);
}

function historyItemsFromResponse(response: any) {
  if (Array.isArray(response?.items)) {
    return response.items as Array<{ message?: any; steps?: any[] }>;
  }
  if (Array.isArray(response?.messages)) {
    const stepsByMessage = new Map<string, any[]>();
    for (const step of response.steps || []) {
      const messageId = step?.messageId ?? step?.message_id;
      if (!messageId) continue;
      const existing = stepsByMessage.get(messageId) ?? [];
      existing.push(step);
      stepsByMessage.set(messageId, existing);
    }
    return response.messages.map((message: any) => ({
      message,
      steps: stepsByMessage.get(message?.id) ?? [],
    }));
  }
  return [];
}

function normalizeHistoryPage(response: any): SessionHistoryPage {
  const items = historyItemsFromResponse(response);
  const rawMessages = items
    .map((item) => item?.message)
    .filter(Boolean)
    .map((message: any, index: number) => ({
      id: stableHistoryMessageId(message, index),
      role: normalizeMessageRole(message.role),
      content: getMessageContent(message),
      parts: Array.isArray(message.parts) ? message.parts : undefined,
      createdAt: message.createdAt ?? message.created_at,
    }));
  const steps = items.flatMap((item) => item?.steps || []);
  return {
    messages: hydrateMessagesWithSteps(rawMessages, steps),
    state: typeof response?.state === "string" ? response.state : "IDLE",
    hasMore: Boolean(response?.hasMore ?? response?.has_more),
    nextBeforeMessageId:
      typeof response?.nextBeforeMessageId === "string"
        ? response.nextBeforeMessageId
        : typeof response?.next_before_message_id === "string"
          ? response.next_before_message_id
          : null,
  };
}

function mergeNewestCanonicalPage(existingMessages: CopilotMessage[], newestPageMessages: CopilotMessage[]) {
  if (newestPageMessages.length === 0) {
    return existingMessages;
  }
  const newestIds = new Set(newestPageMessages.map((message) => message.id));
  const oldestPageId = newestPageMessages[0]?.id;
  const newestPageId = newestPageMessages[newestPageMessages.length - 1]?.id;
  const oldestPageTimestamp = historyMessageTimestamp(newestPageMessages[0]);
  const newestPageTimestamp = historyMessageTimestamp(newestPageMessages[newestPageMessages.length - 1]);
  const preservedOlderMessages = existingMessages.filter((message) => {
    if (message.id === "1") return true;
    if (isLocalMessageId(message.id)) return false;
    if (newestIds.has(message.id)) return false;
    const messageTimestamp = historyMessageTimestamp(message);
    if (messageTimestamp !== null && oldestPageTimestamp !== null) {
      return messageTimestamp < oldestPageTimestamp;
    }
    // Only canonical backend IDs are sortable. Fallback IDs include content/index data and must not order pages.
    return oldestPageId && canCompareCanonicalMessageIds(message.id, oldestPageId) ? message.id < oldestPageId : false;
  });
  const preservedNewerMessages = existingMessages.filter((message) => {
    if (message.id === "1") return false;
    if (isLocalMessageId(message.id)) return false;
    if (newestIds.has(message.id)) return false;
    const messageTimestamp = historyMessageTimestamp(message);
    if (messageTimestamp !== null && newestPageTimestamp !== null) {
      return messageTimestamp > newestPageTimestamp;
    }
    return newestPageId && canCompareCanonicalMessageIds(message.id, newestPageId) ? message.id > newestPageId : false;
  });
  const mergedMessages = [...preservedOlderMessages, ...newestPageMessages, ...preservedNewerMessages];
  const dedupedMessages = new Map<string, CopilotMessage>();
  for (const message of mergedMessages) {
    if (!dedupedMessages.has(message.id)) {
      dedupedMessages.set(message.id, message);
    }
  }
  return Array.from(dedupedMessages.values());
}

export function TalonSession({
  namespace,
  agent,
  gatewayUrl,
  authToken,
  gatewayClient,
  sessionId,
  onSessionChange,
  className,
  style,
  placeholder = "Ask Talon to perform a task...",
  autoFocus = true,
  disabled = false,
  historyPageSize = DEFAULT_HISTORY_PAGE_SIZE,
  historyMessageLimit = DEFAULT_HISTORY_MESSAGE_LIMIT,
  historyStepLimit = DEFAULT_HISTORY_STEP_LIMIT,
}: TalonSessionProps) {
  const [messages, setMessages] = useState<CopilotMessage[]>(emptyMessages);
  const [input, setInput] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [loadingStartedAt, setLoadingStartedAt] = useState<string | number | null>(null);
  const [loadingNow, setLoadingNow] = useState(Date.now());
  const [error, setError] = useState<Error | null>(null);
  const [streamEvents, setStreamEvents] = useState<StreamEventItem[]>([]);
  const [expandedThinkingMessages, setExpandedThinkingMessages] = useState<Record<string, boolean>>({});
  const [expandedToolItems, setExpandedToolItems] = useState<Record<string, boolean>>({});
  const [currentSession, setCurrentSession] = useState<{ ns: string; agent: string; sessionId: string } | null>(null);
  const [hasMoreHistory, setHasMoreHistory] = useState(false);
  const [nextBeforeMessageId, setNextBeforeMessageId] = useState<string | null>(null);
  const [isLoadingOlderHistory, setIsLoadingOlderHistory] = useState(false);
  const [scrollThumb, setScrollThumb] = useState<ScrollThumbState>({ visible: false, top: 0, height: 0 });
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const abortControllerRef = useRef<AbortController | null>(null);
  const resumeAbortControllerRef = useRef<AbortController | null>(null);
  const currentSessionRef = useRef<{ ns: string; agent: string; sessionId: string } | null>(null);
  const messagesRef = useRef<CopilotMessage[]>(emptyMessages);
  const skipNextAutoScrollRef = useRef(false);
  const prependScrollRestoreRef = useRef<{ previousScrollTop: number; previousScrollHeight: number } | null>(null);
  const isLoadingOlderHistoryRef = useRef(false);

  const updateTranscriptScrollThumb = useCallback(() => {
    const container = scrollContainerRef.current;
    if (!container) return;

    const isScrollable = container.scrollHeight > container.clientHeight + 1;
    if (!isScrollable) {
      setScrollThumb((prev) => prev.visible ? { visible: false, top: 0, height: 0 } : prev);
      return;
    }

    const trackInset = 8;
    const trackHeight = Math.max(0, container.clientHeight - trackInset * 2);
    const thumbHeight = Math.max(32, Math.round((container.clientHeight / container.scrollHeight) * trackHeight));
    const maxScrollTop = container.scrollHeight - container.clientHeight;
    const maxThumbTravel = Math.max(0, trackHeight - thumbHeight);
    const scrollRatio = maxScrollTop > 0 ? container.scrollTop / maxScrollTop : 0;
    const next = {
      visible: true,
      top: Math.round(trackInset + maxThumbTravel * scrollRatio),
      height: thumbHeight,
    };

    setScrollThumb((prev) =>
      prev.visible === next.visible && prev.top === next.top && prev.height === next.height ? prev : next,
    );
  }, []);

  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);

  useEffect(() => {
    currentSessionRef.current = currentSession;
  }, [currentSession]);

  const scrollTranscriptToBottom = useCallback((behavior: ScrollBehavior) => {
    const container = scrollContainerRef.current;
    if (container) {
      if (typeof container.scrollTo === "function") {
        container.scrollTo({ top: container.scrollHeight, behavior });
        return;
      }
      container.scrollTop = container.scrollHeight;
    }
    bottomRef.current?.scrollIntoView({ behavior });
  }, []);

  useSafeLayoutEffect(() => {
    const restore = prependScrollRestoreRef.current;
    const container = scrollContainerRef.current;
    if (!restore || !container) return;

    const delta = container.scrollHeight - restore.previousScrollHeight;
    container.scrollTop = restore.previousScrollTop + delta;
    prependScrollRestoreRef.current = null;
    updateTranscriptScrollThumb();
  }, [messages, updateTranscriptScrollThumb]);

  useEffect(() => {
    if (skipNextAutoScrollRef.current) {
      skipNextAutoScrollRef.current = false;
      return;
    }
    const rafId = window.requestAnimationFrame(() => {
      scrollTranscriptToBottom("auto");
      updateTranscriptScrollThumb();
    });
    return () => window.cancelAnimationFrame(rafId);
  }, [currentSession?.sessionId, messages, streamEvents, isLoading, error, scrollTranscriptToBottom, updateTranscriptScrollThumb]);

  useEffect(() => {
    updateTranscriptScrollThumb();
    window.addEventListener("resize", updateTranscriptScrollThumb);
    return () => window.removeEventListener("resize", updateTranscriptScrollThumb);
  }, [updateTranscriptScrollThumb]);

  useSafeLayoutEffect(() => {
    updateTranscriptScrollThumb();
  }, [messages, expandedThinkingMessages, expandedToolItems, isLoading, error, streamEvents, updateTranscriptScrollThumb]);

  useEffect(() => {
    if (!isLoading || loadingStartedAt === null) {
      return;
    }
    setLoadingNow(Date.now());
    const intervalId = window.setInterval(() => setLoadingNow(Date.now()), 250);
    return () => window.clearInterval(intervalId);
  }, [isLoading, loadingStartedAt]);

  useEffect(() => {
    return () => {
      abortControllerRef.current?.abort();
      resumeAbortControllerRef.current?.abort();
    };
  }, []);

  const jsonHeaders = useMemo(() => {
    const headers: HeadersInit = { "Content-Type": "application/json" };
    const authHeaders = buildGatewayHeaders(authToken);
    if (authHeaders?.Authorization) {
      headers.Authorization = authHeaders.Authorization;
    }
    return headers;
  }, [authToken]);

  const inputRows = useMemo(() => {
    let rowCount = 1;
    for (let index = 0; index < input.length; index += 1) {
      if (input.charCodeAt(index) === 10) {
        rowCount += 1;
      }
    }
    return Math.min(rowCount, 8);
  }, [input]);

  const toggleThinkingMessage = useCallback((messageId: string) => {
    setExpandedThinkingMessages((prev) => ({
      ...prev,
      [messageId]: !prev[messageId],
    }));
  }, []);

  const toggleToolItem = useCallback((toolKey: string) => {
    setExpandedToolItems((prev) => ({
      ...prev,
      [toolKey]: !prev[toolKey],
    }));
  }, []);

  const renderedMessages = useMemo(() => {
    return messages.map((message, messageIndex) => {
      const content = getMessageContent(message);
      const timeline = getMessageAssistantTimeline(message);
      const textTimelineItems = timeline.filter((item): item is Extract<AssistantTimelineItem, { type: "text" }> => item.type === "text");
      const toolTimelineItems = timeline.filter((item): item is Extract<AssistantTimelineItem, { type: "tool" }> => item.type === "tool");
      const reasoningContent = getMessageReasoningContent(message);
      const usage = getMessageUsage(message);
      const usageSummary = formatUsageSummary(usage);
      const isUserMessage = message.role === "user";
      const isLatestMessage = messageIndex === messages.length - 1;
      const isStreamingAssistantMessage = isLoading && isLatestMessage && message.role === "assistant" && !content;
      const hasExpandedWorkDetails = Boolean(reasoningContent) || toolTimelineItems.length > 0 || Boolean(usageSummary);
      const hasWorkDetails = message.role === "assistant" && (hasExpandedWorkDetails || isStreamingAssistantMessage);
      const previousUserMessage = message.role === "assistant"
        ? messages.slice(0, messageIndex).reverse().find((previousMessage) => previousMessage.role === "user")
        : undefined;
      const workLabel = isStreamingAssistantMessage
        ? formatWorkingDuration(loadingStartedAt, loadingNow)
        : formatWorkDuration(previousUserMessage?.createdAt, message.createdAt);
      const isWorkExpanded = expandedThinkingMessages[message.id] ?? false;
      return (
        <div
          key={message.id}
          style={{
            display: "flex",
            justifyContent: isUserMessage ? "flex-end" : "stretch",
            width: "100%",
          }}
        >
          <div
            style={{
              width: isUserMessage ? "auto" : "100%",
              maxWidth: isUserMessage ? "min(80%, 36rem)" : "100%",
              overflow: "hidden",
              borderRadius: isUserMessage ? 18 : 0,
              background: isUserMessage
                ? "var(--talon-chat-user-bubble-bg, rgba(24,24,27,0.07))"
                : "transparent",
              color: isUserMessage ? "var(--talon-chat-user-bubble-fg, inherit)" : "inherit",
              padding: isUserMessage ? "0.65rem 0.85rem" : 0,
            }}
          >
            {hasWorkDetails ? (
              <div style={{ marginBottom: 16 }}>
                <button
                  type="button"
                  onClick={() => {
                    if (hasExpandedWorkDetails) {
                      toggleThinkingMessage(message.id);
                    }
                  }}
                  disabled={!hasExpandedWorkDetails}
                  style={{
                    width: "100%",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 12,
                    border: "none",
                    background: "transparent",
                    padding: "0 0 0.65rem",
                    cursor: hasExpandedWorkDetails ? "pointer" : "default",
                    textAlign: "left",
                    color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))",
                  }}
                >
                  <span style={{ fontSize: 13, fontWeight: 500 }}>
                    {workLabel}
                  </span>
                  {hasExpandedWorkDetails ? (
                    <ChevronRight
                      size="16"
                      style={{
                        flexShrink: 0,
                        transform: isWorkExpanded ? "rotate(90deg)" : "rotate(0deg)",
                        transition: "transform 160ms ease",
                        color: "var(--talon-chat-subtle-fg, rgba(113,113,122,0.9))",
                      }}
                    />
                  ) : null}
                </button>
                <div style={{ borderTop: border("var(--talon-chat-divider, rgba(212,212,216,0.7))") }} />

                {isWorkExpanded ? (
                  <div style={{ display: "flex", flexDirection: "column", gap: 6, paddingTop: 12, color: "var(--talon-chat-subtle-fg, rgba(82,82,91,0.96))" }}>
                    {reasoningContent ? (
                      <div style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere", fontSize: 13, lineHeight: 1.55 }}>
                        {reasoningContent}
                      </div>
                    ) : null}

                    {toolTimelineItems.map((item, index) => {
                      const toolKey = `${message.id}-${item.toolCallId || index}`;
                      const isToolExpanded = expandedToolItems[toolKey] ?? false;
                      return (
                        <div key={toolKey}>
                          <button
                            className="talon-session-tool-row"
                            type="button"
                            onClick={() => toggleToolItem(toolKey)}
                            style={{
                              width: "auto",
                              maxWidth: "100%",
                              display: "flex",
                              alignItems: "center",
                              gap: 8,
                              border: "none",
                              background: "transparent",
                              padding: "0.25rem 0",
                              color: "inherit",
                              cursor: "pointer",
                              textAlign: "left",
                            }}
                          >
                            <Wrench size="14" strokeWidth={1.9} style={{ flexShrink: 0, color: "var(--talon-chat-subtle-fg, rgba(113,113,122,0.9))" }} />
                            <span style={{ minWidth: 0, fontSize: 13, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                              Called <span style={{ fontFamily: "ui-monospace, SFMono-Regular, monospace" }}>{item.toolName}</span>
                            </span>
                            <ChevronRight
                              className="talon-session-tool-chevron"
                              size="14"
                              style={{
                                flexShrink: 0,
                                transform: isToolExpanded ? "rotate(90deg)" : "rotate(0deg)",
                                color: "var(--talon-chat-subtle-fg, rgba(113,113,122,0.9))",
                              }}
                            />
                          </button>
                          {isToolExpanded ? (
                            <div style={{ display: "flex", flexDirection: "column", gap: 10, paddingBottom: 12, paddingLeft: 22 }}>
                              <div>
                                <div style={{ marginBottom: 6, fontSize: 11, fontWeight: 700, textTransform: "uppercase", color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>
                                  Input
                                </div>
                                <pre style={{ maxWidth: "100%", overflowX: "auto", whiteSpace: "pre-wrap", overflowWrap: "anywhere", borderRadius: 8, background: "var(--talon-chat-code-bg, rgba(24,24,27,0.05))", padding: 10, fontSize: 12, margin: 0 }}>
                                  <code>{JSON.stringify(item.args ?? {}, null, 2)}</code>
                                </pre>
                              </div>
                              {item.result !== undefined ? (
                                <div>
                                  <div style={{ marginBottom: 6, fontSize: 11, fontWeight: 700, textTransform: "uppercase", color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>
                                    Output
                                  </div>
                                  <pre style={{ maxWidth: "100%", overflowX: "auto", whiteSpace: "pre-wrap", overflowWrap: "anywhere", borderRadius: 8, background: "var(--talon-chat-code-bg, rgba(24,24,27,0.05))", padding: 10, fontSize: 12, margin: 0 }}>
                                    <code>{typeof item.result === "string" ? item.result : JSON.stringify(item.result, null, 2)}</code>
                                  </pre>
                                </div>
                              ) : null}
                            </div>
                          ) : null}
                        </div>
                      );
                    })}

                    {usageSummary ? (
                      <div style={{ fontSize: 12, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>
                        {usageSummary}
                      </div>
                    ) : null}
                  </div>
                ) : null}
              </div>
            ) : null}

            <div
              className={cn(message.role === "system" && "copilot-system-message")}
              style={{
                minWidth: 0,
                overflow: "hidden",
                overflowWrap: "anywhere",
                whiteSpace: message.role === "assistant" ? "normal" : "pre-wrap",
                fontSize: message.role === "system" ? 12 : 14,
                lineHeight: 1.65,
                opacity: message.role === "system" ? 0.72 : 0.94,
                fontFamily: message.role === "system" ? "ui-monospace, SFMono-Regular, monospace" : undefined,
              }}
            >
              {message.role === "assistant" && textTimelineItems.length > 0 ? (
                <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                  {textTimelineItems.map((item, index) => (
                    <div key={`${message.id}-timeline-${index}`} style={{ whiteSpace: "normal", overflowWrap: "anywhere" }}>
                      <MarkdownMessage>{item.text}</MarkdownMessage>
                    </div>
                  ))}
                </div>
              ) : (
                message.role === "assistant" ? <MarkdownMessage>{content}</MarkdownMessage> : content
              )}
            </div>
          </div>
        </div>
      );
    });
  }, [messages, expandedThinkingMessages, expandedToolItems, isLoading, loadingNow, loadingStartedAt, toggleThinkingMessage, toggleToolItem]);

  const resolvedHistoryPageSize = Math.max(
    1,
    Math.trunc(historyPageSize || historyMessageLimit || DEFAULT_HISTORY_PAGE_SIZE),
  );

  const getSessionMessagesPage = useCallback(
    async (target: { ns: string; agent: string; sessionId: string }, beforeMessageId?: string | null) => {
      if (gatewayClient?.listSessionMessages) {
        return gatewayClient.listSessionMessages({
          ...target,
          pageSize: resolvedHistoryPageSize,
          beforeMessageId: beforeMessageId || undefined,
        });
      }

      if (gatewayClient) {
        return gatewayClient.getSession({
          ...target,
          messageLimit: resolvedHistoryPageSize,
          stepLimit: historyStepLimit > 0 ? historyStepLimit : undefined,
        });
      }

      const response = await fetch(
        buildGatewaySessionMessagesUrl(
          gatewayUrl,
          target.ns,
          target.agent,
          target.sessionId,
          resolvedHistoryPageSize,
          beforeMessageId || undefined,
        ),
        { headers: buildGatewayHeaders(authToken) },
      );
      if (!response.ok) {
        throw new Error(`Failed to load session messages: ${response.status}`);
      }
      return response.json();
    },
    [authToken, gatewayClient, gatewayUrl, historyStepLimit, resolvedHistoryPageSize],
  );

  const createSession = useCallback(
    async (target: { ns: string; agent: string }) => {
      if (gatewayClient) {
        return gatewayClient.createSession(target);
      }

      const response = await fetch(
        `${normalizeGatewayUrl(gatewayUrl)}/v1/ns/${encodeURIComponent(target.ns)}/agents/${encodeURIComponent(target.agent)}/sessions`,
        {
          method: "POST",
          headers: jsonHeaders,
          body: JSON.stringify(target),
        },
      );
      if (!response.ok) {
        throw new Error(`Failed to create session: ${response.status}`);
      }
      return response.json();
    },
    [gatewayClient, gatewayUrl, jsonHeaders],
  );

  const loadInitialSessionPage = useCallback(
    async (target: { ns: string; agent: string; sessionId: string }) => {
      const res = normalizeHistoryPage(await getSessionMessagesPage(target));
      setMessages(res.messages);
      setHasMoreHistory(res.hasMore);
      setNextBeforeMessageId(res.nextBeforeMessageId);
      setStreamEvents([]);
      setCurrentSession(target);
      return res;
    },
    [getSessionMessagesPage],
  );

  const loadOlderHistoryPage = useCallback(
    async (target: { ns: string; agent: string; sessionId: string }) => {
      if (!nextBeforeMessageId || isLoadingOlderHistoryRef.current) return;

      const container = scrollContainerRef.current;
      if (container) {
        prependScrollRestoreRef.current = {
          previousScrollTop: container.scrollTop,
          previousScrollHeight: container.scrollHeight,
        };
      }
      skipNextAutoScrollRef.current = true;
      isLoadingOlderHistoryRef.current = true;
      setIsLoadingOlderHistory(true);
      try {
        const res = normalizeHistoryPage(await getSessionMessagesPage(target, nextBeforeMessageId));
        const existingIds = new Set(messagesRef.current.map((message) => message.id));
        const olderMessages = res.messages.filter((message) => !existingIds.has(message.id));
        if (olderMessages.length === 0) {
          prependScrollRestoreRef.current = null;
          skipNextAutoScrollRef.current = false;
        } else {
          setMessages((prev) => {
            const currentIds = new Set(prev.map((message) => message.id));
            const filteredOlderMessages = olderMessages.filter((message) => !currentIds.has(message.id));
            if (filteredOlderMessages.length === 0) {
              return prev;
            }
            return [...filteredOlderMessages, ...prev];
          });
        }
        setHasMoreHistory(res.hasMore);
        setNextBeforeMessageId(res.nextBeforeMessageId);
      } catch (err) {
        prependScrollRestoreRef.current = null;
        skipNextAutoScrollRef.current = false;
        console.warn("Could not load older session history", err);
      } finally {
        isLoadingOlderHistoryRef.current = false;
        setIsLoadingOlderHistory(false);
      }
    },
    [getSessionMessagesPage, nextBeforeMessageId],
  );

  const refreshNewestSessionPage = useCallback(
    async (target: { ns: string; agent: string; sessionId: string }) => {
      const res = normalizeHistoryPage(await getSessionMessagesPage(target));
      const newestPageIds = new Set(res.messages.map((message) => message.id));
      const oldestPageMessage = res.messages[0];
      const oldestPageId = oldestPageMessage?.id;
      const oldestPageTimestamp = oldestPageMessage ? historyMessageTimestamp(oldestPageMessage) : null;
      const hasLoadedOlderHistory = messagesRef.current.some((message) => {
        if (message.id === "1") return false;
        if (isLocalMessageId(message.id)) return false;
        if (newestPageIds.has(message.id)) return false;
        const messageTimestamp = historyMessageTimestamp(message);
        if (messageTimestamp !== null && oldestPageTimestamp !== null) {
          return messageTimestamp < oldestPageTimestamp;
        }
        return oldestPageId && canCompareCanonicalMessageIds(message.id, oldestPageId) ? message.id < oldestPageId : false;
      });
      setMessages((prev) => {
        const merged = mergeNewestCanonicalPage(prev, res.messages);
        return merged;
      });
      if (!hasLoadedOlderHistory) {
        setHasMoreHistory(res.hasMore);
        setNextBeforeMessageId(res.nextBeforeMessageId);
      }
      setStreamEvents([]);
      setCurrentSession(target);
      return res;
    },
    [getSessionMessagesPage],
  );

  const resumeStream = useCallback(
    async (target: { ns: string; agent: string; sessionId: string }, signal?: AbortSignal) => {
      try {
        const response = await fetch(buildGatewayChatUiUrl(gatewayUrl, target.ns, target.agent, target.sessionId), {
          headers: buildGatewayHeaders(authToken),
          signal,
        });
        await streamSessionResume({ response, setMessages, setError, signal });
      } catch (err) {
        if (!signal?.aborted) {
          setError(err instanceof Error ? err : new Error(String(err)));
        }
      }
    },
    [authToken, gatewayUrl],
  );

  useEffect(() => {
    const nextSession = sessionId ? { ns: namespace, agent, sessionId } : null;
    if (!nextSession) {
      if (currentSessionRef.current && currentSessionRef.current.ns === namespace && currentSessionRef.current.agent === agent) {
        return;
      }
      setCurrentSession(null);
      setMessages(emptyMessages);
      setHasMoreHistory(false);
      setNextBeforeMessageId(null);
      setIsLoadingOlderHistory(false);
      setStreamEvents([]);
      setError(null);
      return;
    }

    if (isSameSession(currentSessionRef.current, nextSession)) {
      return;
    }

    let cancelled = false;
    const controller = new AbortController();
    resumeAbortControllerRef.current?.abort();
    resumeAbortControllerRef.current = controller;
    loadInitialSessionPage(nextSession)
      .then((res) => {
        if (!cancelled && res.state === "PROCESSING") {
          void resumeStream(nextSession, controller.signal);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setMessages([{ id: "1", role: "system", content: `[Error loading session history: ${err.message}]` }]);
          setError(err instanceof Error ? err : new Error(String(err)));
        }
      });
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [agent, loadInitialSessionPage, namespace, resumeStream, sessionId]);

  const waitForCanonicalAssistantUpdate = useCallback(
    async (session: { ns: string; agent: string; sessionId: string }, baselineSignature: string) => {
      for (let attempt = 0; attempt < 40; attempt += 1) {
        const sessionState = normalizeHistoryPage(await getSessionMessagesPage(session));
        const nextSignature = getAssistantSignature(sessionState.messages);
        if (nextSignature && nextSignature !== baselineSignature) {
          await refreshNewestSessionPage(session);
          return true;
        }
        await new Promise((resolve) => setTimeout(resolve, 250));
      }
      return false;
    },
    [getSessionMessagesPage, refreshNewestSessionPage],
  );

  const submitMessage = useCallback(async (submittedText: string) => {
    const text = submittedText.trim();
    if (!text || isLoading || disabled) return;

    setInput("");
    setError(null);
    setStreamEvents([]);

    try {
      let session = currentSessionRef.current;
      const baselineAssistantSignature = getAssistantSignature(
        messagesRef.current.slice(-resolvedHistoryPageSize),
      );

      if (!session) {
        const sessionRes = await createSession({ ns: namespace, agent });
        session = { ns: namespace, agent, sessionId: sessionRes.sessionId };
        currentSessionRef.current = session;
        setCurrentSession(session);
        onSessionChange?.(session.sessionId);
      }

      const userMessage: CopilotMessage = {
        id: createLocalMessageId(),
        role: "user",
        content: text,
        parts: [{ type: "text", text }],
        createdAt: String(Date.now() * 1000),
      };

      setMessages((prev) => [...prev, userMessage]);
      setLoadingStartedAt(userMessage.createdAt ?? Date.now());
      setLoadingNow(Date.now());
      setIsLoading(true);

      const controller = new AbortController();
      abortControllerRef.current = controller;

      const response = await fetch(buildGatewayChatUiUrl(gatewayUrl, session.ns, session.agent, session.sessionId), {
        method: "POST",
        headers: jsonHeaders,
        signal: controller.signal,
        body: JSON.stringify({
          messages: [{
            role: userMessage.role,
            content: getMessageContent(userMessage),
            parts: Array.isArray(userMessage.parts) ? userMessage.parts : [{ type: "text", text: getMessageContent(userMessage) }],
          }],
        }),
      });
      if (!response.ok) {
        throw new Error(`Failed to send message: ${response.status}`);
      }

      const { assistantText } = await streamUiSubmission({ response, setMessages, setStreamEvents });

      if (!assistantText) {
        await waitForCanonicalAssistantUpdate(session, baselineAssistantSignature);
      } else {
        await refreshNewestSessionPage(session);
      }
    } catch (err: any) {
      const nextError = err instanceof Error ? err : new Error(String(err));
      const session = currentSessionRef.current;
      if (session) {
        const baselineAssistantSignature = getAssistantSignature(
          messagesRef.current.slice(-resolvedHistoryPageSize),
        );
        const recovered = await waitForCanonicalAssistantUpdate(session, baselineAssistantSignature).catch(() => false);
        if (recovered) {
          setError(null);
          return;
        }
      }
      setError(nextError);
    } finally {
      abortControllerRef.current = null;
      setIsLoading(false);
      setLoadingStartedAt(null);
    }
  }, [agent, createSession, disabled, gatewayUrl, isLoading, jsonHeaders, namespace, onSessionChange, refreshNewestSessionPage, resolvedHistoryPageSize, waitForCanonicalAssistantUpdate]);

  const stopGeneration = useCallback(async () => {
    if (!currentSessionRef.current || !isLoading) return;

    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    setIsLoading(false);
    setLoadingStartedAt(null);

    const session = currentSessionRef.current;
    const response = await fetch(buildGatewayChatUiUrl(gatewayUrl, session.ns, session.agent, session.sessionId), {
      method: "DELETE",
      headers: jsonHeaders,
      body: JSON.stringify(session),
    });

    if (!response.ok) {
      throw new Error(`Failed to stop generation: ${response.status}`);
    }
  }, [gatewayUrl, isLoading, jsonHeaders]);

  const handleTranscriptScroll = useCallback(() => {
    updateTranscriptScrollThumb();
    const container = scrollContainerRef.current;
    const session = currentSessionRef.current;
    if (!container || !session || isLoadingOlderHistoryRef.current || !hasMoreHistory || !nextBeforeMessageId) {
      return;
    }
    if (container.scrollTop <= HISTORY_SCROLL_LOAD_THRESHOLD_PX) {
      void loadOlderHistoryPage(session);
    }
  }, [hasMoreHistory, loadOlderHistoryPage, nextBeforeMessageId, updateTranscriptScrollThumb]);

  return (
    <div
      className={className}
      style={{
        display: "flex",
        flexDirection: "column",
        minWidth: 0,
        minHeight: 0,
        height: "100%",
        background: "transparent",
        color: "inherit",
        fontFamily: talonChatFontFamily,
        ...style,
      }}
    >
      <style>
        {`
          .talon-session-tool-chevron {
            opacity: 0;
            transition: opacity 120ms ease, transform 160ms ease;
          }

          .talon-session-tool-row:hover .talon-session-tool-chevron,
          .talon-session-tool-row:focus-visible .talon-session-tool-chevron {
            opacity: 1;
          }

          .talon-session-transcript {
            scrollbar-width: none;
          }

          .talon-session-transcript::-webkit-scrollbar {
            display: none;
            width: 0;
            height: 0;
          }
        `}
      </style>
      <div style={{ position: "relative", flex: 1, minHeight: 0 }}>
        <div
          className="talon-session-transcript"
          data-testid="copilot-transcript"
          ref={scrollContainerRef}
          onScroll={handleTranscriptScroll}
          style={{ height: "100%", overflowY: "auto", overflowX: "hidden", minHeight: 0 }}
        >
          <div style={{ maxWidth: 896, margin: "0 auto", padding: "1.5rem", display: "flex", flexDirection: "column", gap: "2rem" }}>
          {renderedMessages}

          {isLoading && messages[messages.length - 1]?.role === "user" ? (
            <div style={{ width: "100%" }}>
              <div style={{ fontSize: 13, fontWeight: 500, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>
                {formatWorkingDuration(loadingStartedAt, loadingNow)}
              </div>
            </div>
          ) : null}

          {error ? (
            <div style={{ display: "flex", gap: "1rem" }}>
              <div style={{ flexShrink: 0 }}>
                <div style={{ width: 24, height: 24, borderRadius: 999, display: "flex", alignItems: "center", justifyContent: "center", background: "rgba(254,226,226,1)", border: border("rgba(252,165,165,1)") }}>
                  <Activity size="14" color="rgba(220,38,38,1)" strokeWidth={1.75} />
                </div>
              </div>
              <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 8 }}>
                <span style={{ fontSize: 13, fontWeight: 600, color: "rgba(220,38,38,1)" }}>System Incident</span>
                <div style={{ fontSize: 13, borderRadius: 10, background: "rgba(254,242,242,1)", border: border("rgba(252,165,165,0.6)"), color: "rgba(220,38,38,1)", padding: 12, fontFamily: "ui-monospace, SFMono-Regular, monospace" }}>
                  {error.message || "An error occurred while connecting to the agent."}
                </div>
              </div>
            </div>
          ) : null}
          <div ref={bottomRef} />
          </div>
        </div>
        {scrollThumb.visible ? (
          <div
            aria-hidden="true"
            style={{
              position: "absolute",
              top: scrollThumb.top,
              right: 2,
              width: 5,
              height: scrollThumb.height,
              borderRadius: 999,
              background: "var(--talon-chat-scrollbar-thumb, rgba(113,113,122,0.52))",
              pointerEvents: "none",
            }}
          />
        ) : null}
      </div>

      {disabled ? null : (
        <div
          style={{
            position: "sticky",
            bottom: 0,
            zIndex: 10,
            flexShrink: 0,
            display: "flex",
            justifyContent: "center",
            width: "100%",
            boxSizing: "border-box",
            padding: "1.5rem",
            background: "var(--talon-chat-composer-bg, linear-gradient(to top, rgba(255,255,255,0.94), rgba(255,255,255,0.72) 58%, rgba(255,255,255,0)))",
            backdropFilter: "blur(10px)",
          }}
        >
          <div style={{ width: "100%", maxWidth: 896, paddingBottom: 8 }}>
            <ChatInputBox
              value={input}
              onValueChange={setInput}
              onSubmit={(nextInput) => void submitMessage(nextInput)}
              placeholder={placeholder}
              autoFocus={autoFocus}
              rows={inputRows}
              canSubmit={Boolean((input || "").trim()) && !isLoading}
              isGenerating={isLoading}
              canStop={Boolean(currentSession)}
              onStop={() => {
                void stopGeneration().catch((err: any) =>
                  setError(err instanceof Error ? err : new Error("Failed to stop generation")),
                );
              }}
            />
          </div>
        </div>
      )}
    </div>
  );
}

export const TalonCopilot = TalonSession;
