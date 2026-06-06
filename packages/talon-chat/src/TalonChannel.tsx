"use client";

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Hash } from "lucide-react";
import { buildGatewayHeaders, normalizeGatewayUrl } from "./lib/grpc";
import { ChatInputBox } from "./lib/ChatInputBox";
import { MarkdownMessage } from "./lib/MarkdownMessage";

function border(color: string) {
  return `1px solid ${color}`;
}

const talonChatFontFamily =
  'var(--talon-chat-font-family, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif)';

const CHANNEL_SCROLL_LOAD_THRESHOLD_PX = 64;
const CHANNEL_SCROLL_BOTTOM_THRESHOLD_PX = 96;

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

export type UseTalonChannelMessagesOptions = {
  namespace: string;
  channel: string | ChannelLike | null | undefined;
  gatewayUrl: string;
  authToken?: string | null;
  disabled?: boolean;
  messageLimit?: number;
  refreshIntervalMs?: number | false;
};

export type UseTalonChannelMessagesResult = {
  channelName: string;
  status: string;
  messages: ChannelMessage[];
  isLoading: boolean;
  isLoadingOlderMessages: boolean;
  hasMoreMessages: boolean;
  error: string | null;
  refresh: (options?: { silent?: boolean; replace?: boolean }) => Promise<void>;
  loadOlderMessages: () => Promise<void>;
  postMessage: (options: { author: string; authorKind: string; content: string }) => Promise<void>;
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
  if (normalized >= 1e14) return Math.trunc(normalized / 1000);
  if (normalized >= 1e11) return Math.trunc(normalized);
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

function isNearScrollBottom(container: HTMLElement) {
  return container.scrollHeight - container.scrollTop - container.clientHeight <= CHANNEL_SCROLL_BOTTOM_THRESHOLD_PX;
}

function mergeChannelMessages(existing: ChannelMessage[], incoming: ChannelMessage[]) {
  const byKey = new Map<string, ChannelMessage>();
  existing.forEach((message, index) => byKey.set(channelMessageKey(message, index), message));
  incoming.forEach((message, index) => byKey.set(channelMessageKey(message, existing.length + index), message));
  return Array.from(byKey.values()).sort(compareChannelMessages);
}

export function useTalonChannelMessages({
  namespace,
  channel,
  gatewayUrl,
  authToken,
  disabled = false,
  messageLimit = 100,
  refreshIntervalMs = 2000,
}: UseTalonChannelMessagesOptions): UseTalonChannelMessagesResult {
  const [messages, setMessages] = useState<ChannelMessage[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [isLoadingOlderMessages, setIsLoadingOlderMessages] = useState(false);
  const [hasMoreMessages, setHasMoreMessages] = useState(false);
  const [nextBeforeMessageId, setNextBeforeMessageId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pendingRefreshRef = useRef(false);
  const refreshRequestIdRef = useRef(0);
  const messagesRef = useRef<ChannelMessage[]>([]);
  const isLoadingOlderMessagesRef = useRef(false);
  const delayedRefreshTimeoutRef = useRef<number | null>(null);
  const loadedChannelRef = useRef<{ namespace: string; channelName: string } | null>(null);
  const refreshConfigVersionRef = useRef(0);

  const channelName = coerceChannelName(channel);
  const status = coerceChannelStatus(channel);
  const currentChannelRef = useRef({ namespace, channelName });

  useEffect(() => {
    currentChannelRef.current = { namespace, channelName };
  }, [namespace, channelName]);

  useEffect(() => {
    refreshConfigVersionRef.current += 1;
    refreshRequestIdRef.current += 1;
    pendingRefreshRef.current = false;
    setIsLoading(false);
  }, [authToken, disabled, gatewayUrl, messageLimit]);

  const headers = useCallback(
    (json = false): HeadersInit => ({
      ...(json ? { "Content-Type": "application/json" } : {}),
      ...(buildGatewayHeaders(authToken) || {}),
    }),
    [authToken],
  );

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

  const refresh = useCallback(
    async (options?: { silent?: boolean; replace?: boolean }) => {
      if (!namespace || !channelName || disabled || pendingRefreshRef.current) return;
      const requestNamespace = namespace;
      const requestChannelName = channelName;
      const requestConfigVersion = refreshConfigVersionRef.current;
      const requestId = refreshRequestIdRef.current + 1;
      refreshRequestIdRef.current = requestId;
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
          requestChannelName !== currentChannelRef.current.channelName ||
          requestConfigVersion !== refreshConfigVersionRef.current
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
          requestChannelName === currentChannelRef.current.channelName &&
          requestConfigVersion === refreshConfigVersionRef.current
        ) {
          setError(err?.message || "Failed to load channel");
        }
      } finally {
        if (
          requestNamespace === currentChannelRef.current.namespace &&
          requestChannelName === currentChannelRef.current.channelName &&
          requestConfigVersion === refreshConfigVersionRef.current &&
          requestId === refreshRequestIdRef.current
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
    const previousChannel = loadedChannelRef.current;
    const channelChanged =
      !previousChannel ||
      previousChannel.namespace !== namespace ||
      previousChannel.channelName !== channelName;
    loadedChannelRef.current = { namespace, channelName };
    refreshRequestIdRef.current += 1;
    pendingRefreshRef.current = false;
    if (channelChanged) {
      setMessages([]);
      messagesRef.current = [];
      setHasMoreMessages(false);
      setNextBeforeMessageId(null);
      setIsLoading(false);
      setIsLoadingOlderMessages(false);
      setError(null);
      isLoadingOlderMessagesRef.current = false;
    }
    void refresh({ replace: true, silent: !channelChanged });
  }, [namespace, channelName, refresh]);

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
      setMessages((existing) => mergeChannelMessages(existing, page.messages));
      setHasMoreMessages(page.hasMore);
      setNextBeforeMessageId(page.nextBeforeMessageId);
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

  const postMessage = useCallback(
    async ({ author, authorKind, content }: { author: string; authorKind: string; content: string }) => {
      const trimmedContent = content.trim();
      if (!trimmedContent || !namespace || !channelName || disabled || status === "closed") return;
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
              content: trimmedContent,
            }),
          },
        );
        if (!response.ok) throw new Error(`Post HTTP ${response.status}`);
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
        throw err;
      }
    },
    [channelName, disabled, gatewayUrl, headers, namespace, refresh, status],
  );

  return {
    channelName,
    status,
    messages,
    isLoading,
    isLoadingOlderMessages,
    hasMoreMessages,
    error,
    refresh,
    loadOlderMessages,
    postMessage,
  };
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
  const [draft, setDraft] = useState("");
  const [isPosting, setIsPosting] = useState(false);
  const isPostingRef = useRef(false);
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const skipNextAutoScrollRef = useRef(false);
  const isNearBottomRef = useRef(true);
  const {
    channelName,
    status,
    messages,
    isLoading,
    isLoadingOlderMessages,
    hasMoreMessages,
    error,
    loadOlderMessages,
    postMessage,
  } = useTalonChannelMessages({
    namespace,
    channel,
    gatewayUrl,
    authToken,
    disabled,
    messageLimit,
    refreshIntervalMs,
  });
  const isUserInputDisabled = disabled || disableUserInput || status === "closed";

  const resolvedFormatTimestamp = useMemo(() => {
    if (formatTimestamp) return formatTimestamp;
    return (message: ChannelMessage) => defaultFormatTimestamp(message, timestampLocale);
  }, [formatTimestamp, timestampLocale]);

  const scrollMessagesToBottom = useCallback((behavior: ScrollBehavior) => {
    const container = scrollContainerRef.current;
    if (!container) return;
    if (typeof container.scrollTo === "function") {
      container.scrollTo({ top: container.scrollHeight, behavior });
      return;
    }
    container.scrollTop = container.scrollHeight;
  }, []);

  const canPost = Boolean(draft.trim()) && !isPosting && !isUserInputDisabled;

  useEffect(() => {
    isNearBottomRef.current = true;
  }, [namespace, channelName]);

  useEffect(() => {
    if (skipNextAutoScrollRef.current) {
      skipNextAutoScrollRef.current = false;
      return;
    }
    if (messages.length > 0 && !isNearBottomRef.current) return;
    const rafId = window.requestAnimationFrame(() => {
      scrollMessagesToBottom("auto");
      isNearBottomRef.current = true;
    });
    return () => window.cancelAnimationFrame(rafId);
  }, [messages, scrollMessagesToBottom]);

  const handleMessageScroll = useCallback((event: React.UIEvent<HTMLDivElement>) => {
    isNearBottomRef.current = isNearScrollBottom(event.currentTarget);
    if (!hasMoreMessages || isLoadingOlderMessages) return;
    if (event.currentTarget.scrollTop > CHANNEL_SCROLL_LOAD_THRESHOLD_PX) return;
    const container = event.currentTarget;
    const previousScrollHeight = container.scrollHeight;
    const previousScrollTop = container.scrollTop;
    skipNextAutoScrollRef.current = true;
    void loadOlderMessages().then(() => {
      window.requestAnimationFrame(() => {
        const nextContainer = scrollContainerRef.current;
        if (!nextContainer) return;
        nextContainer.scrollTop = nextContainer.scrollHeight - previousScrollHeight + previousScrollTop;
      });
    });
  }, [hasMoreMessages, isLoadingOlderMessages, loadOlderMessages]);

  const submitChannelMessage = useCallback(async (submittedContent: string) => {
    const content = submittedContent.trim();
    if (!content || isPostingRef.current || isUserInputDisabled) return;
    isPostingRef.current = true;
    setIsPosting(true);
    try {
      await postMessage({ author, authorKind, content });
      setDraft("");
    } catch {
      // The hook owns the visible error state; keep the draft so the operator can retry.
    } finally {
      isPostingRef.current = false;
      setIsPosting(false);
    }
  }, [author, authorKind, isUserInputDisabled, postMessage]);

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
        fontFamily: talonChatFontFamily,
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
            <div style={{ marginBottom: 12, borderRadius: 10, border: border("var(--copilot-channel-error-border, rgba(248,113,113,0.5))"), background: "var(--copilot-channel-error-bg, rgba(248,113,113,0.12))", color: "var(--copilot-channel-error-fg, inherit)", padding: 12, fontSize: 13 }}>
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
                  <div key={channelMessageKey(message, index)} style={{ borderRadius: 12, border: border("rgba(148,163,184,0.24)"), background: "var(--copilot-channel-message-bg, rgba(255,255,255,0.72))", color: "var(--copilot-channel-message-fg, inherit)", padding: "1rem" }}>
                    <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 8, fontSize: 12, opacity: 0.72 }}>
                      <span style={{ display: "inline-flex", alignItems: "center", gap: 6, fontWeight: 700, color: "inherit", opacity: 1 }}>
                        <Hash size="13" />
                        {messageAuthorKind}:{message.author || "unknown"}
                      </span>
                      <span style={{ fontFamily: "ui-monospace, SFMono-Regular, monospace" }}>{resolvedFormatTimestamp(message)}</span>
                      {messageActions ? <div style={{ marginLeft: "auto" }}>{messageActions}</div> : null}
                    </div>
                    <div style={{ marginTop: 8, whiteSpace: "normal", overflowWrap: "anywhere", fontSize: 14, lineHeight: 1.6 }}>
                      <MarkdownMessage>{message.content || ""}</MarkdownMessage>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {disableUserInput ? null : (
          <div style={{ borderTop: border("rgba(148,163,184,0.2)"), background: "var(--copilot-channel-input-bg, rgba(255,255,255,0.72))", padding: "0.75rem" }}>
            <ChatInputBox
              value={draft}
              onValueChange={setDraft}
              onSubmit={(content) => void submitChannelMessage(content)}
              placeholder={`Message #${channelName}`}
              rows={1}
              disabled={isUserInputDisabled}
              canSubmit={canPost}
              textareaMinHeight={40}
              textareaMaxHeight={128}
            />
          </div>
        )}
      </div>
    </div>
  );
}
