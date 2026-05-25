"use client";

import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Activity, ChevronRight, Send, Square, User } from "lucide-react";
import { Streamdown } from "streamdown";
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
import { buildGatewayHeaders, normalizeGatewayUrl } from "./lib/grpc";
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

export type TalonCopilotProps = {
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
  talonIcon?: React.ReactNode;
  timestampLocale?: Intl.LocalesArgument;
  formatTimestamp?: (message: CopilotMessage) => string;
  historyPageSize?: number;
  historyMessageLimit?: number;
  historyStepLimit?: number;
};

const bootMessage: CopilotMessage[] = [
  { id: "1", role: "system", content: "Talon runtime initialized." },
];
const DEFAULT_HISTORY_PAGE_SIZE = 50;
const DEFAULT_HISTORY_MESSAGE_LIMIT = 100;
const DEFAULT_HISTORY_STEP_LIMIT = 1000;
const HISTORY_SCROLL_LOAD_THRESHOLD_PX = 120;

function DefaultTalonIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 1000 1000" fill="none" aria-hidden="true">
      <rect width="1000" height="1000" fill="#09090B" />
      <g stroke="#5B8CFF" strokeWidth="80" fill="none" strokeLinejoin="miter" strokeLinecap="butt">
        <path d="M330 500L670 500" />
        <path d="M500 250L500 750" />
        <path d="M330 333.33L330 500" />
        <path d="M670 333.33L670 500" />
      </g>
      <path d="M296.91 477.50L363.09 522.50L216.40 750L119.60 750L296.91 477.50Z" fill="#5B8CFF" />
      <path d="M636.91 522.50L703.09 477.50L880.40 750L783.60 750L636.91 522.50Z" fill="#5B8CFF" />
    </svg>
  );
}

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

function defaultFormatMessageTimestamp(message: CopilotMessage, timestampLocale?: Intl.LocalesArgument) {
  function formatTimestampValue(value: unknown) {
    const timestampMs = normalizeEpochToMilliseconds(value);
    if (timestampMs === null) {
      return "—";
    }
    return new Date(timestampMs).toLocaleString(timestampLocale, {
      year: "numeric",
      month: "numeric",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
      second: "2-digit",
      hour12: true,
    });
  }

  function millisecondsFromUuidLike(id: unknown) {
    if (typeof id !== "string") return null;
    const compactHex = id.replace(/[^0-9a-fA-F]/g, "");
    if (compactHex.length >= 32 && compactHex.charAt(12) === "7") {
      const time = parseInt(compactHex.slice(0, 12), 16);
      return Number.isNaN(time) ? null : time;
    }
    return null;
  }

  function millisecondsFromUlidLike(id: unknown) {
    if (typeof id !== "string") return null;
    const value = id.trim().toUpperCase();
    if (!/^[0-9A-HJKMNP-TV-Z]{26}$/.test(value)) {
      return null;
    }
    const alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let timestampMs = 0;
    for (const char of value.slice(0, 10)) {
      const index = alphabet.indexOf(char);
      if (index < 0) return null;
      timestampMs = (timestampMs * 32) + index;
    }
    return Number.isFinite(timestampMs) && timestampMs > 0 ? timestampMs : null;
  }

  const explicit = message?.createdAt ?? (message as CopilotMessage & { created_at?: string | number | bigint }).created_at;
  if (explicit !== undefined && explicit !== null && explicit !== "") {
    return formatTimestampValue(explicit);
  }
  const inferred = millisecondsFromUuidLike(message?.id) ?? millisecondsFromUlidLike(message?.id);
  return inferred ? formatTimestampValue(inferred) : "—";
}

function MarkdownMessage({ children }: { children: string }) {
  const compactListChildren = (content: React.ReactNode): React.ReactNode =>
    React.Children.map(content, (child) => {
      if (!React.isValidElement(child)) {
        return child;
      }

      const elementChild = child as React.ReactElement<any>;
      const nextChildren = elementChild.props?.children ? compactListChildren(elementChild.props.children) : elementChild.props?.children;

      if (child.type === "p") {
        const paragraphChild = child as React.ReactElement<{ style?: React.CSSProperties; children?: React.ReactNode }>;
        return React.createElement("span", {
          style: {
            ...(paragraphChild.props.style || {}),
            margin: 0,
            display: "inline",
          },
          children: nextChildren,
        });
      }

      if (child.type === "br") {
        return null;
      }

      if (child.type === "ul" || child.type === "ol") {
        return React.cloneElement(child as React.ReactElement<any>, {
          style: {
            ...(elementChild.props.style || {}),
            marginTop: 0,
            marginBottom: "0.5rem",
            paddingLeft: "1.05rem",
          },
          children: nextChildren,
        });
      }

      if (typeof child.type === "string") {
        const nextStyle =
          child.type === "li"
            ? {
                ...(elementChild.props.style || {}),
                marginTop: 0,
                marginBottom: "0.25rem",
                lineHeight: 1.5,
              }
              : elementChild.props.style;

        return React.cloneElement(child as React.ReactElement<any>, {
          ...(nextStyle ? { style: nextStyle } : {}),
          ...(nextChildren !== undefined ? { children: nextChildren } : {}),
        });
      }

      return React.cloneElement(child as React.ReactElement<any>, {
        ...(nextChildren !== undefined ? { children: nextChildren } : {}),
      });
    });

  return (
    <div style={{ minWidth: 0, lineHeight: 1.6 }}>
      <Streamdown
        components={{
          p: (props) => <p {...props} style={{ margin: "0 0 0.45rem" }} />,
          ul: (props) => <ul {...props} style={{ margin: "0.25rem 0 0.45rem", paddingLeft: "1.05rem", lineHeight: 1.5 }} />,
          ol: (props) => <ol {...props} style={{ margin: "0.25rem 0 0.45rem", paddingLeft: "1.05rem", lineHeight: 1.5 }} />,
          li: (props) => (
            <li {...props} style={{ margin: "0 0 0.25rem", paddingLeft: "0.08rem", lineHeight: 1.5 }}>
              {compactListChildren(props.children)}
            </li>
          ),
          h1: (props) => <h1 {...props} style={{ margin: "0.7rem 0 0.35rem", fontSize: "1.3em", fontWeight: 700, lineHeight: 1.3 }} />,
          h2: (props) => <h2 {...props} style={{ margin: "0.6rem 0 0.3rem", fontSize: "1.18em", fontWeight: 700, lineHeight: 1.35 }} />,
          h3: (props) => <h3 {...props} style={{ margin: "0.5rem 0 0.25rem", fontSize: "1.08em", fontWeight: 700, lineHeight: 1.35 }} />,
          pre: (props) => (
            <pre
              {...props}
              style={{
                margin: "0.55rem 0 0.7rem",
                padding: "0.75rem",
                overflowX: "auto",
                borderRadius: 12,
                border: border("rgba(148,163,184,0.24)"),
                background: "rgba(148,163,184,0.08)",
              }}
            />
          ),
          code: (props) => (
            <code
              {...props}
              style={{
                fontFamily: "ui-monospace, SFMono-Regular, monospace",
                fontSize: "0.92em",
              }}
            />
          ),
          a: (props) => <a {...props} style={{ color: "inherit", textDecoration: "underline" }} />,
          blockquote: (props) => (
            <blockquote
              {...props}
              style={{
                margin: "0.5rem 0 0.65rem",
                paddingLeft: "0.65rem",
                borderLeft: border("rgba(148,163,184,0.4)"),
                opacity: 0.88,
              }}
            />
          ),
        }}
      >
        {children}
      </Streamdown>
    </div>
  );
}

function getAssistantSignature(messages: Array<{ role?: unknown; id?: unknown; content?: unknown }> | undefined) {
  if (!Array.isArray(messages)) return "";
  return messages
    .filter((message) => message?.role === "assistant" || message?.role === 2 || message?.role === "ROLE_ASSISTANT")
    .map((message) => `${String(message.id ?? "")}:${String(message.content ?? "").length}`)
    .join("|");
}

type SessionHistoryPage = {
  messages: CopilotMessage[];
  state: string;
  hasMore: boolean;
  nextBeforeMessageId: string | null;
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
  const content = typeof message?.content === "string" ? message.content : "";
  return `history-${role}-${createdAt}-${index}-${stableStringHash(content)}`;
}

function historyMessageTimestamp(message: Pick<CopilotMessage, "createdAt">) {
  return normalizeEpochToMilliseconds(message.createdAt);
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
      content: message.content,
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

export function TalonCopilot({
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
  talonIcon = <DefaultTalonIcon />,
  timestampLocale,
  formatTimestamp,
  historyPageSize = DEFAULT_HISTORY_PAGE_SIZE,
  historyMessageLimit = DEFAULT_HISTORY_MESSAGE_LIMIT,
  historyStepLimit = DEFAULT_HISTORY_STEP_LIMIT,
}: TalonCopilotProps) {
  const [messages, setMessages] = useState<CopilotMessage[]>(bootMessage);
  const [input, setInput] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const [streamEvents, setStreamEvents] = useState<StreamEventItem[]>([]);
  const [expandedThinkingMessages, setExpandedThinkingMessages] = useState<Record<string, boolean>>({});
  const [expandedToolItems, setExpandedToolItems] = useState<Record<string, boolean>>({});
  const [currentSession, setCurrentSession] = useState<{ ns: string; agent: string; sessionId: string } | null>(null);
  const [hasMoreHistory, setHasMoreHistory] = useState(false);
  const [nextBeforeMessageId, setNextBeforeMessageId] = useState<string | null>(null);
  const [isLoadingOlderHistory, setIsLoadingOlderHistory] = useState(false);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const transcriptContentRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const abortControllerRef = useRef<AbortController | null>(null);
  const resumeAbortControllerRef = useRef<AbortController | null>(null);
  const currentSessionRef = useRef<{ ns: string; agent: string; sessionId: string } | null>(null);
  const messagesRef = useRef<CopilotMessage[]>(bootMessage);
  const skipNextAutoScrollRef = useRef(false);
  const prependScrollRestoreRef = useRef<{ previousScrollTop: number; previousScrollHeight: number } | null>(null);
  const isLoadingOlderHistoryRef = useRef(false);

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
  }, [messages]);

  useEffect(() => {
    if (skipNextAutoScrollRef.current) {
      skipNextAutoScrollRef.current = false;
      return;
    }
    const rafId = window.requestAnimationFrame(() => {
      scrollTranscriptToBottom("auto");
    });
    return () => window.cancelAnimationFrame(rafId);
  }, [currentSession?.sessionId, messages, streamEvents, isLoading, error, scrollTranscriptToBottom]);

  useEffect(() => {
    const content = transcriptContentRef.current;
    if (!content || typeof ResizeObserver === "undefined") {
      return;
    }

    const observer = new ResizeObserver(() => {
      if (skipNextAutoScrollRef.current || prependScrollRestoreRef.current) {
        return;
      }
      scrollTranscriptToBottom("auto");
    });
    observer.observe(content);
    return () => observer.disconnect();
  }, [currentSession?.sessionId, scrollTranscriptToBottom]);

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

  const resolvedTimestampFormatter = useMemo(() => {
    if (formatTimestamp) {
      return formatTimestamp;
    }
    return (message: CopilotMessage) => defaultFormatMessageTimestamp(message, timestampLocale);
  }, [formatTimestamp, timestampLocale]);

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
    return messages.map((message) => {
      const content = getMessageContent(message);
      const timeline = getMessageAssistantTimeline(message);
      const reasoningContent = getMessageReasoningContent(message);
      const usage = getMessageUsage(message);
      const usageSummary = formatUsageSummary(usage);
      return (
        <div key={message.id} style={{ display: "flex", gap: "1rem" }}>
          <div style={{ flexShrink: 0, marginTop: 2 }}>
            {message.role === "user" ? (
              <div style={{ width: 24, height: 24, borderRadius: 8, display: "flex", alignItems: "center", justifyContent: "center", background: "rgba(148,163,184,0.16)", border: border("rgba(148,163,184,0.24)") }}>
                <User size="14" strokeWidth={1.75} />
              </div>
            ) : (
              <div style={{ width: 24, height: 24, borderRadius: 8, display: "flex", alignItems: "center", justifyContent: "center", background: "currentColor", color: "var(--copilot-inverse-color, #fff)" }}>
                {talonIcon}
              </div>
            )}
          </div>

          <div style={{ flex: 1, overflow: "hidden", display: "flex", flexDirection: "column", gap: 8 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span style={{ fontSize: 13, fontWeight: 600 }}>{message.role === "user" ? "Operator" : "Talon"}</span>
              <span style={{ fontSize: 11, opacity: 0.64, fontFamily: "ui-monospace, SFMono-Regular, monospace" }}>
                {resolvedTimestampFormatter(message)}
              </span>
            </div>

            {reasoningContent ? (
              <div style={{ paddingBottom: 8 }}>
                <button
                  type="button"
                  onClick={() => toggleThinkingMessage(message.id)}
                  style={{
                    width: "100%",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    borderRadius: 12,
                    border: border("rgba(245,158,11,0.28)"),
                    background: "rgba(251,191,36,0.1)",
                    padding: "0.625rem 0.75rem",
                    cursor: "pointer",
                    textAlign: "left",
                  }}
                >
                  <span style={{ fontSize: 12, fontWeight: 600, color: "rgba(180,83,9,1)" }}>
                    Thinking{usageSummary ? ` • ${usageSummary}` : ""}
                  </span>
                  <ChevronRight
                    size="16"
                    style={{
                      transform: expandedThinkingMessages[message.id] ? "rotate(90deg)" : "rotate(0deg)",
                      transition: "transform 160ms ease",
                      color: "rgba(180,83,9,0.8)",
                    }}
                  />
                </button>

                {expandedThinkingMessages[message.id] ? (
                  <div style={{ marginTop: 12, borderRadius: 12, border: border("rgba(245,158,11,0.24)"), background: "rgba(251,191,36,0.06)", padding: 12 }}>
                    <div style={{ marginBottom: 8, fontSize: 11, textTransform: "uppercase", letterSpacing: "0.08em", color: "rgba(180,83,9,0.8)" }}>
                      Raw Reasoning
                    </div>
                    <div style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere", fontFamily: "ui-monospace, SFMono-Regular, monospace", fontSize: 12, lineHeight: 1.6 }}>
                      {reasoningContent}
                    </div>
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
              {message.role === "assistant" && timeline.length > 0 ? (
                <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                  {timeline.map((item: AssistantTimelineItem, index: number) =>
                    item.type === "text" ? (
                      <div key={`${message.id}-timeline-${index}`} style={{ whiteSpace: "normal", overflowWrap: "anywhere" }}>
                        <MarkdownMessage>{item.text}</MarkdownMessage>
                      </div>
                    ) : (() => {
                      const toolKey = `${message.id}-${item.toolCallId ?? index}`;
                      const isExpanded = expandedToolItems[toolKey] ?? false;
                      return (
                        <div key={toolKey} style={{ borderRadius: 16, border: border("rgba(148,163,184,0.24)"), background: "rgba(148,163,184,0.08)", padding: 12 }}>
                          <button
                            type="button"
                            onClick={() => toggleToolItem(toolKey)}
                            style={{
                              width: "100%",
                              display: "flex",
                              alignItems: "center",
                              justifyContent: "space-between",
                              gap: 12,
                              background: "transparent",
                              border: "none",
                              padding: 0,
                              cursor: "pointer",
                              textAlign: "left",
                            }}
                          >
                            <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, fontWeight: 600, minWidth: 0 }}>
                              <span style={{ borderRadius: 999, background: "rgba(255,255,255,0.74)", padding: "2px 8px", fontSize: 11, textTransform: "uppercase", letterSpacing: "0.08em", color: "rgba(100,116,139,1)" }}>
                                Tool
                              </span>
                              <span style={{ fontFamily: "ui-monospace, SFMono-Regular, monospace", overflow: "hidden", textOverflow: "ellipsis" }}>{item.toolName}</span>
                            </div>
                            <ChevronRight
                              size="16"
                              style={{
                                flexShrink: 0,
                                transform: isExpanded ? "rotate(90deg)" : "rotate(0deg)",
                                transition: "transform 160ms ease",
                                color: "rgba(100,116,139,0.8)",
                              }}
                            />
                          </button>
                          {isExpanded ? (
                            <>
                              <div style={{ marginTop: 12, marginBottom: 8, fontSize: 11, textTransform: "uppercase", letterSpacing: "0.08em", color: "rgba(100,116,139,1)" }}>
                                Arguments
                              </div>
                              <pre style={{ maxWidth: "100%", overflowX: "auto", whiteSpace: "pre-wrap", overflowWrap: "anywhere", borderRadius: 10, border: border("rgba(148,163,184,0.24)"), background: "rgba(255,255,255,0.72)", padding: 12, fontSize: 12 }}>
                                <code>{JSON.stringify(item.args ?? {}, null, 2)}</code>
                              </pre>
                              {item.result !== undefined ? (
                                <>
                                  <div style={{ marginTop: 12, marginBottom: 8, fontSize: 11, textTransform: "uppercase", letterSpacing: "0.08em", color: "rgba(100,116,139,1)" }}>
                                    Result
                                  </div>
                                  <pre style={{ maxWidth: "100%", overflowX: "auto", whiteSpace: "pre-wrap", overflowWrap: "anywhere", borderRadius: 10, border: border("rgba(148,163,184,0.24)"), background: "rgba(255,255,255,0.72)", padding: 12, fontSize: 12 }}>
                                    <code>{typeof item.result === "string" ? item.result : JSON.stringify(item.result, null, 2)}</code>
                                  </pre>
                                </>
                              ) : null}
                            </>
                          ) : null}
                        </div>
                      );
                    })(),
                  )}
                </div>
              ) : (
                message.role === "assistant" ? <MarkdownMessage>{content}</MarkdownMessage> : content
              )}
            </div>
          </div>
        </div>
      );
    });
  }, [messages, expandedThinkingMessages, expandedToolItems, resolvedTimestampFormatter, talonIcon, toggleThinkingMessage, toggleToolItem]);

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
      setMessages(res.messages.length > 0 ? res.messages : bootMessage);
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
        return merged.length > 0 ? merged : bootMessage;
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
      setMessages(bootMessage);
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

  const getLatestStatus = useCallback(() => {
    const reasoningItems = streamEvents.filter((item) => item.type === "reasoning");
    if (reasoningItems.length > 0) {
      return "Thinking";
    }
    const statusItems = streamEvents.filter((item) => item.type === "status");
    if (statusItems.length > 0) {
      return statusItems[statusItems.length - 1].content;
    }
    const latestToolCall = streamEvents.filter((item) => item.type === "tool_call").at(-1);
    if (latestToolCall?.name) {
      return `Calling ${latestToolCall.name}`;
    }
    return "Thinking...";
  }, [streamEvents]);

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
    }
  }, [agent, createSession, disabled, gatewayUrl, isLoading, jsonHeaders, namespace, onSessionChange, refreshNewestSessionPage, resolvedHistoryPageSize, waitForCanonicalAssistantUpdate]);

  const stopGeneration = useCallback(async () => {
    if (!currentSessionRef.current || !isLoading) return;

    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    setIsLoading(false);

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
    const container = scrollContainerRef.current;
    const session = currentSessionRef.current;
    if (!container || !session || isLoadingOlderHistory || !hasMoreHistory || !nextBeforeMessageId) {
      return;
    }
    if (container.scrollTop <= HISTORY_SCROLL_LOAD_THRESHOLD_PX) {
      void loadOlderHistoryPage(session);
    }
  }, [hasMoreHistory, isLoadingOlderHistory, loadOlderHistoryPage, nextBeforeMessageId]);

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
        ...style,
      }}
    >
      <div ref={scrollContainerRef} onScroll={handleTranscriptScroll} style={{ flex: 1, overflowY: "auto", overflowX: "hidden", minHeight: 0 }}>
        <div style={{ maxWidth: 896, margin: "0 auto", padding: "1rem 1rem 2rem", display: "flex", flexDirection: "column", gap: "2rem" }}>
          {renderedMessages}

          {isLoading && (messages[messages.length - 1]?.role === "user" || (messages[messages.length - 1]?.role === "assistant" && !messages[messages.length - 1]?.content)) ? (
            <div style={{ display: "flex", gap: "1rem" }}>
              <div style={{ flexShrink: 0, marginTop: 2 }}>
                <div style={{ width: 24, height: 24, borderRadius: 8, display: "flex", alignItems: "center", justifyContent: "center", background: "currentColor", color: "var(--copilot-inverse-color, #fff)" }}>
                  {talonIcon}
                </div>
              </div>
              <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 8 }}>
                <span style={{ fontSize: 13, fontWeight: 600 }}>Talon</span>
                <div style={{ fontSize: 12, opacity: 0.7, fontFamily: "ui-monospace, SFMono-Regular, monospace" }}>
                  ⏳ {getLatestStatus()}
                </div>
                <div style={{ display: "flex", alignItems: "center", gap: 6, height: 24 }}>
                  <div style={{ width: 6, height: 6, borderRadius: 999, background: "rgba(15,23,42,0.3)" }} />
                  <div style={{ width: 6, height: 6, borderRadius: 999, background: "rgba(15,23,42,0.4)" }} />
                  <div style={{ width: 6, height: 6, borderRadius: 999, background: "rgba(15,23,42,0.5)" }} />
                </div>
              </div>
            </div>
          ) : null}

          {error ? (
            <div style={{ display: "flex", gap: "1rem" }}>
              <div style={{ flexShrink: 0, marginTop: 2 }}>
                <div style={{ width: 24, height: 24, borderRadius: 8, display: "flex", alignItems: "center", justifyContent: "center", background: "rgba(254,226,226,1)", border: border("rgba(252,165,165,1)") }}>
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

      <div
        style={{
          position: "sticky",
          bottom: 0,
          zIndex: 10,
          flexShrink: 0,
          display: "flex",
          justifyContent: "center",
          width: "100%",
          padding: "0.85rem 1rem 1rem",
          background: "linear-gradient(to top, rgba(255,255,255,0.94), rgba(255,255,255,0.72) 58%, rgba(255,255,255,0))",
          backdropFilter: "blur(10px)",
        }}
      >
        <div style={{ width: "100%", maxWidth: 896, paddingBottom: 8 }}>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void submitMessage(input);
            }}
            style={{
              position: "relative",
              display: "flex",
              alignItems: "flex-end",
              gap: 8,
              borderRadius: 22,
              border: border("rgba(148,163,184,0.28)"),
              background: "var(--copilot-input-bg, rgba(255,255,255,0.96))",
              boxShadow: "0 4px 14px rgba(15,23,42,0.08), 0 1px 2px rgba(15,23,42,0.06)",
              padding: "0.5rem 0.5rem 0.5rem 0.75rem",
              backdropFilter: "blur(12px)",
            }}
          >
            <textarea
              value={input}
              onChange={(event) => setInput(event.target.value)}
              placeholder={placeholder}
              autoFocus={autoFocus}
              disabled={disabled}
              rows={inputRows}
              style={{
                flex: 1,
                resize: "none",
                border: "none",
                outline: "none",
                boxShadow: "none",
                background: "transparent",
                padding: "0.5rem",
                maxHeight: "40vh",
                minHeight: 24,
                fontSize: 15,
                lineHeight: 1.6,
                overflowY: "auto",
                color: "inherit",
                appearance: "none",
                WebkitAppearance: "none",
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  if ((event.currentTarget.value || "").trim() && !isLoading && !disabled) {
                    void submitMessage(event.currentTarget.value);
                  }
                }
              }}
            />
            <button
              type={isLoading ? "button" : "submit"}
              onClick={
                isLoading
                  ? () => {
                      void stopGeneration().catch((err: any) =>
                        setError(err instanceof Error ? err : new Error("Failed to stop generation")),
                      );
                    }
                  : undefined
              }
              disabled={isLoading ? !currentSession : !(input || "").trim() || disabled}
              aria-label={isLoading ? "Stop generation" : "Send message"}
              style={{
                width: 40,
                height: 40,
                flexShrink: 0,
                borderRadius: 14,
                border: "none",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                cursor: isLoading || ((input || "").trim() && !disabled) ? "pointer" : "not-allowed",
                opacity: isLoading || ((input || "").trim() && !disabled) ? 1 : 0.5,
                background: "currentColor",
                color: "var(--copilot-inverse-color, #fff)",
              }}
            >
              {isLoading ? <Square size="16" strokeWidth={2} fill="currentColor" /> : <Send size="16" strokeWidth={2} />}
            </button>
          </form>
          <div style={{ textAlign: "center", marginTop: 12, fontSize: 11, opacity: 0.6 }}>
            Press Return to send, Shift + Return for new line
          </div>
        </div>
      </div>
    </div>
  );
}
