"use client";

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Hash, Send } from "lucide-react";
import { buildGatewayHeaders, normalizeGatewayUrl } from "./lib/grpc";

function border(color: string) {
  return `1px solid ${color}`;
}

const activeControlBackground = "var(--copilot-control-bg, var(--foreground, #020617))";
const activeControlColor = "var(--copilot-control-fg, var(--background, #ffffff))";
const CHANNEL_SCROLL_LOAD_THRESHOLD_PX = 64;

type ChannelLike = {
  name?: string;
  ns?: string;
  title?: string;
  status?: string;
  metadata?: Record<string, string>;
  labels?: Record<string, string>;
};

export type ChannelMessage = {
  id?: string;
  ns?: string;
  channel?: string;
  authorKind?: string;
  author_kind?: string;
  author?: string;
  content?: string;
  createdAt?: bigint | number | string;
  created_at?: bigint | number | string;
  sourceAgent?: string;
  source_agent?: string;
  sourceSessionId?: string;
  source_session_id?: string;
};

export type TalonChannelProps = {
  namespace: string;
  channel: string | ChannelLike | null | undefined;
  gatewayUrl: string;
  authToken?: string | null;
  className?: string;
  style?: React.CSSProperties;
  disabled?: boolean;
  disableUserInput?: boolean;
  author?: string;
  authorKind?: string;
  messageLimit?: number;
  refreshIntervalMs?: number | false;
  timestampLocale?: Intl.LocalesArgument;
  formatTimestamp?: (message: ChannelMessage) => string;
  renderMessageActions?: (message: ChannelMessage) => React.ReactNode;
};

function coerceChannelName(channel: string | ChannelLike | null | undefined) {
  if (!channel) return "";
  return typeof channel === "string" ? channel : channel.name || "";
}

function coerceChannelStatus(channel: string | ChannelLike | null | undefined) {
  if (!channel) return "open";
  return typeof channel === "string" ? "open" : channel.status || "open";
}

function normalizeEpochToMilliseconds(value: unknown) {
  let normalized: number | null = null;
  if (typeof value === "bigint") {
    const bigintValue = value < BigInt(0) ? -value : value;
    if (bigintValue > BigInt(Number.MAX_SAFE_INTEGER)) return null;
    normalized = Number(value);
  } else if (typeof value === "string") {
    const numericValue = Number(value);
    normalized = Number.isFinite(numericValue) ? numericValue : Date.parse(value);
  } else if (typeof value === "number") {
    normalized = value;
  }
  if (typeof normalized !== "number" || !Number.isFinite(normalized) || normalized <= 0) return null;
  if (normalized >= 1e15) return Math.trunc(normalized / 1000);
  if (normalized >= 1e12) return Math.trunc(normalized);
  if (normalized >= 1e9) return Math.trunc(normalized * 1000);
  return null;
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

function defaultFormatTimestamp(message: ChannelMessage, timestampLocale?: Intl.LocalesArgument) {
  const explicit = message.createdAt ?? message.created_at;
  const timestampMs = normalizeEpochToMilliseconds(explicit) ?? millisecondsFromUuidLike(message.id);
  if (timestampMs === null) return "-";
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

function buildGatewayChannelMessagesUrl(
  gatewayUrl: string,
  namespace: string,
  channel: string,
  pageSize: number,
  beforeMessageId?: string,
) {
  const url = new URL(`${normalizeGatewayUrl(gatewayUrl)}/v1/ns/${encodeURIComponent(namespace)}/channels/${encodeURIComponent(channel)}/messages`);
  url.searchParams.set("page_size", String(Math.trunc(pageSize)));
  if (beforeMessageId) {
    url.searchParams.set("before_message_id", beforeMessageId);
  }
  return url.toString();
}

function normalizeChannelPage(response: any): {
  messages: ChannelMessage[];
  hasMore: boolean;
  nextBeforeMessageId: string | null;
} {
  return {
    messages: Array.isArray(response?.messages) ? response.messages : [],
    hasMore: Boolean(response?.hasMore ?? response?.has_more),
    nextBeforeMessageId:
      typeof response?.nextBeforeMessageId === "string"
        ? response.nextBeforeMessageId
        : typeof response?.next_before_message_id === "string"
          ? response.next_before_message_id
          : null,
  };
}

function channelMessageTimestamp(message: ChannelMessage) {
  return normalizeEpochToMilliseconds(message.createdAt ?? message.created_at) ?? millisecondsFromUuidLike(message.id);
}

function channelMessageKey(message: ChannelMessage, fallbackIndex: number) {
  return message.id || `${message.createdAt ?? message.created_at ?? fallbackIndex}:${message.author || ""}:${message.content || ""}`;
}

function compareChannelMessages(left: ChannelMessage, right: ChannelMessage) {
  const leftTimestamp = channelMessageTimestamp(left);
  const rightTimestamp = channelMessageTimestamp(right);
  if (leftTimestamp !== null && rightTimestamp !== null && leftTimestamp !== rightTimestamp) {
    return leftTimestamp - rightTimestamp;
  }
  if (left.id && right.id && left.id !== right.id) {
    return left.id < right.id ? -1 : 1;
  }
  return 0;
}

function mergeChannelMessages(existing: ChannelMessage[], incoming: ChannelMessage[]) {
  const byKey = new Map<string, ChannelMessage>();
  existing.forEach((message, index) => byKey.set(channelMessageKey(message, index), message));
  incoming.forEach((message, index) => byKey.set(channelMessageKey(message, existing.length + index), message));
  return Array.from(byKey.values()).sort(compareChannelMessages);
}

export function TalonChannel({
  namespace,
  channel,
  gatewayUrl,
  authToken,
  className,
  style,
  disabled = false,
  disableUserInput = false,
  author = "sightline",
  authorKind = "user",
  messageLimit = 100,
  refreshIntervalMs = 2000,
  timestampLocale,
  formatTimestamp,
  renderMessageActions,
}: TalonChannelProps) {
  const [messages, setMessages] = useState<ChannelMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [isPosting, setIsPosting] = useState(false);
  const [isLoadingOlderMessages, setIsLoadingOlderMessages] = useState(false);
  const [hasMoreMessages, setHasMoreMessages] = useState(false);
  const [nextBeforeMessageId, setNextBeforeMessageId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const pendingRefreshRef = useRef(false);
  const messagesRef = useRef<ChannelMessage[]>([]);
  const isLoadingOlderMessagesRef = useRef(false);
  const skipNextAutoScrollRef = useRef(false);
  const delayedRefreshTimeoutRef = useRef<number | null>(null);

  const channelName = coerceChannelName(channel);
  const status = coerceChannelStatus(channel);
  const isUserInputDisabled = disabled || disableUserInput || status === "closed";
  const currentChannelRef = useRef({ namespace, channelName });

  useEffect(() => {
    currentChannelRef.current = { namespace, channelName };
  }, [namespace, channelName]);

  const headers = useCallback(
    (json = false): HeadersInit => ({
      ...(json ? { "Content-Type": "application/json" } : {}),
      ...(buildGatewayHeaders(authToken) || {}),
    }),
    [authToken],
  );

  const resolvedFormatTimestamp = useMemo(() => {
    if (formatTimestamp) return formatTimestamp;
    return (message: ChannelMessage) => defaultFormatTimestamp(message, timestampLocale);
  }, [formatTimestamp, timestampLocale]);

  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);

  useEffect(() => {
    return () => {
      if (delayedRefreshTimeoutRef.current !== null) {
        window.clearTimeout(delayedRefreshTimeoutRef.current);
        delayedRefreshTimeoutRef.current = null;
      }
    };
  }, [namespace, channelName]);

  const scrollMessagesToBottom = useCallback((behavior: ScrollBehavior) => {
    const container = scrollContainerRef.current;
    if (!container) return;
    if (typeof container.scrollTo === "function") {
      container.scrollTo({ top: container.scrollHeight, behavior });
      return;
    }
    container.scrollTop = container.scrollHeight;
  }, []);

  useEffect(() => {
    if (skipNextAutoScrollRef.current) {
      skipNextAutoScrollRef.current = false;
      return;
    }
    const rafId = window.requestAnimationFrame(() => {
      scrollMessagesToBottom("auto");
    });
    return () => window.cancelAnimationFrame(rafId);
  }, [messages, isLoading, error, scrollMessagesToBottom]);

  const refresh = useCallback(
    async (options?: { silent?: boolean; replace?: boolean }) => {
      if (!namespace || !channelName || disabled || pendingRefreshRef.current) return;
      const requestNamespace = namespace;
      const requestChannelName = channelName;
      pendingRefreshRef.current = true;
      if (!options?.silent) {
        setIsLoading(true);
      }
      setError(null);
      try {
        const messagesResponse = await fetch(
          buildGatewayChannelMessagesUrl(gatewayUrl, requestNamespace, requestChannelName, messageLimit),
          { headers: headers() },
        );
        if (!messagesResponse.ok) throw new Error(`Messages HTTP ${messagesResponse.status}`);
        const responseJson = await messagesResponse.json();
        if (
          requestNamespace !== currentChannelRef.current.namespace ||
          requestChannelName !== currentChannelRef.current.channelName
        ) {
          return;
        }
        const page = normalizeChannelPage(responseJson);
        const newestIds = new Set(page.messages.map((message, index) => channelMessageKey(message, index)));
        const oldestNewestMessage = page.messages[0];
        const oldestNewestTimestamp = oldestNewestMessage ? channelMessageTimestamp(oldestNewestMessage) : null;
        const hasLoadedOlderMessages = messagesRef.current.some((message, index) => {
          const key = channelMessageKey(message, index);
          if (newestIds.has(key)) return false;
          const messageTimestamp = channelMessageTimestamp(message);
          if (messageTimestamp !== null && oldestNewestTimestamp !== null) {
            return messageTimestamp < oldestNewestTimestamp;
          }
          return Boolean(message.id && oldestNewestMessage?.id && message.id < oldestNewestMessage.id);
        });
        setMessages((existing) => options?.replace ? page.messages : mergeChannelMessages(existing, page.messages));
        if (options?.replace || !hasLoadedOlderMessages) {
          setHasMoreMessages(page.hasMore);
          setNextBeforeMessageId(page.nextBeforeMessageId);
        }
      } catch (err: any) {
        if (
          requestNamespace === currentChannelRef.current.namespace &&
          requestChannelName === currentChannelRef.current.channelName
        ) {
          setError(err?.message || "Failed to load channel");
        }
      } finally {
        if (
          requestNamespace === currentChannelRef.current.namespace &&
          requestChannelName === currentChannelRef.current.channelName
        ) {
          pendingRefreshRef.current = false;
          if (!options?.silent) {
            setIsLoading(false);
          }
        }
      }
    },
    [channelName, disabled, gatewayUrl, headers, messageLimit, namespace],
  );

  useEffect(() => {
    setMessages([]);
    messagesRef.current = [];
    setHasMoreMessages(false);
    setNextBeforeMessageId(null);
    setIsLoading(false);
    setIsLoadingOlderMessages(false);
    setError(null);
    isLoadingOlderMessagesRef.current = false;
    skipNextAutoScrollRef.current = false;
    pendingRefreshRef.current = false;
    void refresh({ replace: true });
  }, [namespace, channelName]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (refreshIntervalMs === false || disabled || !namespace || !channelName) return;
    const intervalMs = Math.max(750, Math.trunc(refreshIntervalMs));
    const timer = window.setInterval(() => {
      void refresh({ silent: true });
    }, intervalMs);
    return () => window.clearInterval(timer);
  }, [channelName, disabled, namespace, refresh, refreshIntervalMs]);

  const loadOlderMessages = useCallback(async () => {
    if (!namespace || !channelName || disabled || !hasMoreMessages || !nextBeforeMessageId || isLoadingOlderMessagesRef.current) return;
    const requestNamespace = namespace;
    const requestChannelName = channelName;
    isLoadingOlderMessagesRef.current = true;
    setIsLoadingOlderMessages(true);
    setError(null);
    try {
      const response = await fetch(
        buildGatewayChannelMessagesUrl(gatewayUrl, requestNamespace, requestChannelName, messageLimit, nextBeforeMessageId),
        { headers: headers() },
      );
      if (!response.ok) throw new Error(`Messages HTTP ${response.status}`);
      const responseJson = await response.json();
      if (
        requestNamespace !== currentChannelRef.current.namespace ||
        requestChannelName !== currentChannelRef.current.channelName
      ) {
        return;
      }
      const page = normalizeChannelPage(responseJson);
      skipNextAutoScrollRef.current = true;
      const container = scrollContainerRef.current;
      const previousScrollHeight = container?.scrollHeight ?? 0;
      const previousScrollTop = container?.scrollTop ?? 0;
      setMessages((existing) => mergeChannelMessages(existing, page.messages));
      setHasMoreMessages(page.hasMore);
      setNextBeforeMessageId(page.nextBeforeMessageId);
      window.requestAnimationFrame(() => {
        const nextContainer = scrollContainerRef.current;
        if (!nextContainer) return;
        nextContainer.scrollTop = nextContainer.scrollHeight - previousScrollHeight + previousScrollTop;
      });
    } catch (err: any) {
      if (
        requestNamespace === currentChannelRef.current.namespace &&
        requestChannelName === currentChannelRef.current.channelName
      ) {
        setError(err?.message || "Failed to load older channel messages");
      }
    } finally {
      if (
        requestNamespace === currentChannelRef.current.namespace &&
        requestChannelName === currentChannelRef.current.channelName
      ) {
        isLoadingOlderMessagesRef.current = false;
        setIsLoadingOlderMessages(false);
      }
    }
  }, [channelName, disabled, gatewayUrl, hasMoreMessages, headers, messageLimit, namespace, nextBeforeMessageId]);

  const handleMessageScroll = useCallback((event: React.UIEvent<HTMLDivElement>) => {
    if (!hasMoreMessages || isLoadingOlderMessagesRef.current || !nextBeforeMessageId) return;
    if (event.currentTarget.scrollTop <= CHANNEL_SCROLL_LOAD_THRESHOLD_PX) {
      void loadOlderMessages();
    }
  }, [hasMoreMessages, loadOlderMessages, nextBeforeMessageId]);

  const postMessage = useCallback(
    async (event: React.FormEvent) => {
      event.preventDefault();
      const content = draft.trim();
      if (!content || !namespace || !channelName || isUserInputDisabled) return;
      setIsPosting(true);
      setError(null);
      try {
        const response = await fetch(
          `${normalizeGatewayUrl(gatewayUrl)}/v1/ns/${encodeURIComponent(namespace)}/channels/${encodeURIComponent(channelName)}/messages`,
          {
            method: "POST",
            headers: headers(true),
            body: JSON.stringify({
              ns: namespace,
              channel: channelName,
              authorKind,
              author,
              content,
            }),
          },
        );
        if (!response.ok) throw new Error(`Post HTTP ${response.status}`);
        setDraft("");
        await refresh();
        if (delayedRefreshTimeoutRef.current !== null) {
          window.clearTimeout(delayedRefreshTimeoutRef.current);
        }
        delayedRefreshTimeoutRef.current = window.setTimeout(() => {
          delayedRefreshTimeoutRef.current = null;
          void refresh({ silent: true });
        }, 1000);
      } catch (err: any) {
        setError(err?.message || "Failed to post channel message");
      } finally {
        setIsPosting(false);
      }
    },
    [author, authorKind, channelName, draft, gatewayUrl, headers, isUserInputDisabled, namespace, refresh],
  );

  const canPost = Boolean(draft.trim()) && !isPosting && !isUserInputDisabled;

  return (
    <div
      className={className}
      style={{
        display: "flex",
        flexDirection: "column",
        minWidth: 0,
        minHeight: 0,
        height: "100%",
        overflow: "hidden",
        background: "transparent",
        color: "inherit",
        ...style,
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", flex: 1, minHeight: 0 }}>
        <div
          ref={scrollContainerRef}
          aria-label="Channel messages"
          onScroll={handleMessageScroll}
          style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "1rem" }}
        >
          {isLoading ? <div style={{ marginBottom: 12, fontSize: 12, opacity: 0.68 }}>Loading channel...</div> : null}
          {isLoadingOlderMessages ? <div style={{ marginBottom: 12, fontSize: 12, opacity: 0.68 }}>Loading older messages...</div> : null}
          {error ? (
            <div style={{ marginBottom: 12, borderRadius: 10, border: border("rgba(252,165,165,0.6)"), background: "rgba(254,242,242,0.82)", color: "rgb(185,28,28)", padding: 12, fontSize: 13 }}>
              {error}
            </div>
          ) : null}
          {messages.length === 0 && !isLoading ? (
            <div style={{ fontSize: 14, opacity: 0.68 }}>No channel messages.</div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              {messages.map((message, index) => {
                const messageAuthorKind = message.authorKind || message.author_kind || "user";
                const messageActions = renderMessageActions?.(message);
                return (
                  <div key={channelMessageKey(message, index)} style={{ borderRadius: 12, border: border("rgba(148,163,184,0.24)"), background: "rgba(255,255,255,0.72)", padding: "1rem" }}>
                    <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 8, fontSize: 12, opacity: 0.72 }}>
                      <span style={{ display: "inline-flex", alignItems: "center", gap: 6, fontWeight: 700, color: "inherit", opacity: 1 }}>
                        <Hash size="13" />
                        {messageAuthorKind}:{message.author || "unknown"}
                      </span>
                      <span style={{ fontFamily: "ui-monospace, SFMono-Regular, monospace" }}>{resolvedFormatTimestamp(message)}</span>
                      {messageActions ? <div style={{ marginLeft: "auto" }}>{messageActions}</div> : null}
                    </div>
                    <div style={{ marginTop: 8, whiteSpace: "pre-wrap", overflowWrap: "anywhere", fontSize: 14, lineHeight: 1.6 }}>
                      {message.content || ""}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {disableUserInput ? null : (
          <form onSubmit={postMessage} style={{ display: "flex", alignItems: "flex-end", gap: 8, borderTop: border("rgba(148,163,184,0.2)"), background: "rgba(255,255,255,0.72)", padding: "0.75rem" }}>
            <textarea
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              placeholder={`Message #${channelName}`}
              rows={1}
              disabled={isUserInputDisabled}
              style={{
                flex: 1,
                minHeight: 40,
                maxHeight: 128,
                resize: "none",
                borderRadius: 10,
                border: border("rgba(148,163,184,0.28)"),
                background: "rgba(255,255,255,0.9)",
                padding: "0.55rem 0.7rem",
                fontSize: 14,
                color: "inherit",
                outline: "none",
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  if (canPost) {
                    event.currentTarget.form?.requestSubmit();
                  }
                }
              }}
            />
            <button
              type="submit"
              disabled={!canPost}
              aria-label="Send channel message"
              style={{
                width: 40,
                height: 40,
                flexShrink: 0,
                borderRadius: 12,
                border: "none",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                cursor: canPost ? "pointer" : "not-allowed",
                opacity: canPost ? 1 : 0.5,
                background: activeControlBackground,
                color: activeControlColor,
              }}
            >
              <Send size="16" strokeWidth={2} />
            </button>
          </form>
        )}
      </div>
    </div>
  );
}
