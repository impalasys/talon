"use client";

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Hash, Send } from "lucide-react";
import { buildGatewayHeaders, normalizeGatewayUrl } from "./lib/grpc";

function border(color: string) {
  return `1px solid ${color}`;
}

const activeControlBackground = "var(--copilot-control-bg, var(--foreground, #020617))";
const activeControlColor = "var(--copilot-control-fg, var(--background, #ffffff))";

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
  channel: string | ChannelLike;
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

function coerceChannelName(channel: string | ChannelLike) {
  return typeof channel === "string" ? channel : channel.name || "";
}

function coerceChannelStatus(channel: string | ChannelLike) {
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
  const [error, setError] = useState<string | null>(null);
  const pendingRefreshRef = useRef(false);

  const channelName = coerceChannelName(channel);
  const status = coerceChannelStatus(channel);
  const isUserInputDisabled = disabled || disableUserInput || status === "closed";

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

  const refresh = useCallback(
    async (options?: { silent?: boolean }) => {
      if (!namespace || !channelName || disabled || pendingRefreshRef.current) return;
      pendingRefreshRef.current = true;
      if (!options?.silent) {
        setIsLoading(true);
      }
      setError(null);
      try {
        const baseUrl = normalizeGatewayUrl(gatewayUrl);
        const encodedNs = encodeURIComponent(namespace);
        const encodedChannel = encodeURIComponent(channelName);
        const messagesResponse = await fetch(`${baseUrl}/v1/ns/${encodedNs}/channels/${encodedChannel}/messages?limit=${encodeURIComponent(String(messageLimit))}`, {
          headers: headers(),
        });
        if (!messagesResponse.ok) throw new Error(`Messages HTTP ${messagesResponse.status}`);
        const messagesPayload = await messagesResponse.json();
        setMessages(Array.isArray(messagesPayload.messages) ? messagesPayload.messages : []);
      } catch (err: any) {
        setError(err?.message || "Failed to load channel");
      } finally {
        pendingRefreshRef.current = false;
        if (!options?.silent) {
          setIsLoading(false);
        }
      }
    },
    [channelName, disabled, gatewayUrl, headers, messageLimit, namespace],
  );

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (refreshIntervalMs === false || disabled || !namespace || !channelName) return;
    const intervalMs = Math.max(750, Math.trunc(refreshIntervalMs));
    const timer = window.setInterval(() => {
      void refresh({ silent: true });
    }, intervalMs);
    return () => window.clearInterval(timer);
  }, [channelName, disabled, namespace, refresh, refreshIntervalMs]);

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
        window.setTimeout(() => {
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
        <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "1rem" }}>
          {isLoading ? <div style={{ marginBottom: 12, fontSize: 12, opacity: 0.68 }}>Loading channel...</div> : null}
          {error ? (
            <div style={{ marginBottom: 12, borderRadius: 10, border: border("rgba(252,165,165,0.6)"), background: "rgba(254,242,242,0.82)", color: "rgb(185,28,28)", padding: 12, fontSize: 13 }}>
              {error}
            </div>
          ) : null}
          {messages.length === 0 && !isLoading ? (
            <div style={{ fontSize: 14, opacity: 0.68 }}>No channel messages.</div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              {messages.map((message) => {
                const messageAuthorKind = message.authorKind || message.author_kind || "user";
                const messageActions = renderMessageActions?.(message);
                return (
                  <div key={message.id} style={{ borderRadius: 12, border: border("rgba(148,163,184,0.24)"), background: "rgba(255,255,255,0.72)", padding: "1rem" }}>
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
