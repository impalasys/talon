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
import {
  findTalonChatCommand,
  parseTalonChatCommandInput,
  type TalonBuiltInCommandName,
  type TalonChatCommand,
} from "./lib/commands";
import { MarkdownMessage } from "./lib/MarkdownMessage";
import { streamSessionPartEvents, type StreamEventItem } from "./lib/uiStream";

const useSafeLayoutEffect = typeof window !== "undefined" ? useLayoutEffect : useEffect;

export type SessionServiceClientLike = Pick<
  {
    create(request: { ns: string; agent: string; labels?: Record<string, string> }): Promise<{ sessionId: string }>;
    clear(request: { ns: string; agent: string; sessionId: string }): Promise<any>;
    listMessages(request: {
      ns: string;
      agent: string;
      sessionId: string;
      pageSize: number;
      beforeMessageId?: string;
    }): Promise<any>;
    submitTurn(request: any, options?: { signal?: AbortSignal }): AsyncIterable<any>;
    streamParts(request: { ns: string; agent: string; sessionId: string }, options?: { signal?: AbortSignal }): AsyncIterable<any>;
    stopGeneration(request: { ns: string; agent: string; sessionId: string }): Promise<any>;
  },
  "create" | "clear" | "listMessages" | "submitTurn" | "streamParts" | "stopGeneration"
>;

export type GatewayClientLike = {
  sessions: SessionServiceClientLike;
};

export type TalonSessionCommandTarget = {
  type: "session";
  namespace: string;
  agent: string;
  sessionId: string | null;
};

export type TalonSessionCommand = TalonChatCommand<TalonSessionCommandTarget, CopilotMessage>;

export type TalonChatObjectRef = {
  key: string;
  mediaType?: string;
  media_type?: string;
  sizeBytes?: number | bigint | string;
  size_bytes?: number | bigint | string;
  sha256?: string;
  filename?: string;
  metadata?: Record<string, string>;
};

export type TalonImageUploadContext = {
  file: File;
  namespace: string;
  agent: string;
  sessionId: string;
  signal: AbortSignal;
};

export type TalonImageUploadResult = TalonChatObjectRef | {
  object: TalonChatObjectRef;
  url?: string;
};

export type TalonSessionProps = {
  namespace: string;
  agent: string;
  gatewayClient: GatewayClientLike;
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
  commands?: TalonSessionCommand[];
  enabledBuiltInCommands?: TalonBuiltInCommandName[];
  /**
   * Uploads an image selected in the composer and returns the stored object ref.
   * TalonSession performs client-side type and size checks for UX only; callers
   * must validate file type, size, and content again in this upload handler
   * before storing or processing the file.
   */
  onImageUpload?: (context: TalonImageUploadContext) => Promise<TalonImageUploadResult>;
  objectUrlForRef?: (object: TalonChatObjectRef) => string | undefined;
  maxImageAttachments?: number;
  /**
   * Client-side image size limit in bytes. This improves UX only and must be
   * enforced again by the onImageUpload implementation.
   */
  maxImageBytes?: number;
  /**
   * Client-side accepted image MIME types. This can be bypassed by callers and
   * must be enforced again by the onImageUpload implementation.
   */
  acceptedImageTypes?: string[];
};

export type TalonCopilotProps = TalonSessionProps;

const emptyMessages: CopilotMessage[] = [];
const DEFAULT_HISTORY_PAGE_SIZE = 50;
const DEFAULT_HISTORY_MESSAGE_LIMIT = 100;
const DEFAULT_HISTORY_STEP_LIMIT = 1000;
const HISTORY_SCROLL_LOAD_THRESHOLD_PX = 120;
const SESSION_MESSAGE_PART_TYPE_IMAGE = 7;

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

type PendingImageAttachment = {
  id: string;
  file: File;
  previewUrl: string;
  object?: TalonChatObjectRef;
  status: "queued" | "uploading" | "ready" | "error";
  error?: string;
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

function objectRefMediaType(object: TalonChatObjectRef | undefined) {
  return object?.mediaType || object?.media_type || "";
}

function objectRefSizeBytes(object: TalonChatObjectRef): number {
  const value = object.sizeBytes ?? object.size_bytes ?? 0;
  if (typeof value === "bigint") return Number(value);
  if (typeof value === "string") {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : 0;
  }
  return Number.isFinite(value) ? value : 0;
}

function normalizeObjectRefForJson(object: TalonChatObjectRef) {
  return {
    key: object.key,
    mediaType: object.mediaType ?? object.media_type ?? "",
    sizeBytes: objectRefSizeBytes(object),
    sha256: object.sha256 ?? "",
    filename: object.filename ?? "",
    metadata: object.metadata ?? {},
  };
}

function normalizeImageUploadResult(result: TalonImageUploadResult) {
  return "object" in result ? result.object : result;
}

function serializableMessageParts(parts: unknown) {
  if (!Array.isArray(parts)) return [];
  return parts.map((part: any) => {
    if (!part || typeof part !== "object") return part;
    const { previewUrl: _previewUrl, ...serializablePart } = part;
    return serializablePart;
  });
}

function protoSessionPartsFromChatParts(parts: unknown) {
  return serializableMessageParts(parts).map((part: any) => {
    if (part?.type === "image") {
      return {
        partType: SESSION_MESSAGE_PART_TYPE_IMAGE,
        payloadJson: part.payloadJson ?? part.payload_json ?? "",
        object: part.object,
      };
    }
    return {
      partType: 1,
      content: String(part?.text ?? part?.content ?? ""),
    };
  });
}

function parsePayloadJson(payloadJson: unknown): Record<string, unknown> {
  if (typeof payloadJson !== "string" || payloadJson.length === 0) return {};
  try {
    const value = JSON.parse(payloadJson);
    return value && typeof value === "object" ? value as Record<string, unknown> : {};
  } catch {
    return {};
  }
}

function objectRefFromPart(part: any): TalonChatObjectRef | undefined {
  const object = part?.object ?? part?.objectRef ?? part?.object_ref;
  return object && typeof object === "object" ? object as TalonChatObjectRef : undefined;
}

function messageImageParts(
  message: CopilotMessage,
  objectUrlForRef?: (object: TalonChatObjectRef) => string | undefined,
): Array<{ id: string; src?: string; label: string }> {
  if (!Array.isArray(message.parts)) return [];
  return message.parts.flatMap((part: any, index) => {
    const type = part?.type ?? part?.partType ?? part?.part_type;
    if (type !== "image" && type !== SESSION_MESSAGE_PART_TYPE_IMAGE && type !== "SESSION_MESSAGE_PART_TYPE_IMAGE") {
      return [];
    }
    const payload = parsePayloadJson(part.payloadJson ?? part.payload_json);
    const object = objectRefFromPart(part);
    const src =
      typeof part.previewUrl === "string"
        ? part.previewUrl
        : typeof part.url === "string"
          ? part.url
          : typeof payload.url === "string"
            ? payload.url
            : object
              ? objectUrlForRef?.(object)
              : undefined;
    const label =
      object?.filename ||
      (typeof payload.filename === "string" ? payload.filename : undefined) ||
      object?.key ||
      `image-${index + 1}`;
    return [{ id: `${message.id}-image-${index}`, src, label }];
  });
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
  commands,
  enabledBuiltInCommands,
  onImageUpload,
  objectUrlForRef,
  maxImageAttachments = 4,
  maxImageBytes = 20 * 1024 * 1024,
  acceptedImageTypes = ["image/png", "image/jpeg", "image/gif", "image/webp"],
}: TalonSessionProps) {
  const [messages, setMessages] = useState<CopilotMessage[]>(emptyMessages);
  const [input, setInput] = useState("");
  const [imageAttachments, setImageAttachments] = useState<PendingImageAttachment[]>([]);
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
  const imageAttachmentsRef = useRef<PendingImageAttachment[]>([]);
  const submittedPreviewUrlsRef = useRef<string[]>([]);
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
    imageAttachmentsRef.current = imageAttachments;
  }, [imageAttachments]);

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
      for (const attachment of imageAttachmentsRef.current) {
        URL.revokeObjectURL(attachment.previewUrl);
      }
      for (const previewUrl of submittedPreviewUrlsRef.current) {
        URL.revokeObjectURL(previewUrl);
      }
    };
  }, []);

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
      const images = messageImageParts(message, objectUrlForRef);
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
      let previousUserMessage: CopilotMessage | undefined;
      if (message.role === "assistant") {
        for (let index = messageIndex - 1; index >= 0; index -= 1) {
          if (messages[index].role === "user") {
            previousUserMessage = messages[index];
            break;
          }
        }
      }
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
            {images.length > 0 ? (
              <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginTop: content ? 10 : 0 }}>
                {images.map((image) => (
                  image.src ? (
                    <img
                      key={image.id}
                      src={image.src}
                      alt={image.label}
                      style={{
                        width: 132,
                        maxWidth: "100%",
                        aspectRatio: "1 / 1",
                        objectFit: "cover",
                        borderRadius: 8,
                        border: border("var(--talon-chat-image-border, rgba(212,212,216,0.86))"),
                      }}
                    />
                  ) : (
                    <div
                      key={image.id}
                      title={image.label}
                      style={{
                        maxWidth: "100%",
                        borderRadius: 8,
                        border: border("var(--talon-chat-image-border, rgba(212,212,216,0.86))"),
                        padding: "0.45rem 0.6rem",
                        fontSize: 12,
                        lineHeight: 1.3,
                        color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))",
                        overflowWrap: "anywhere",
                      }}
                    >
                      {image.label}
                    </div>
                  )
                ))}
              </div>
            ) : null}
          </div>
        </div>
      );
    });
  }, [messages, expandedThinkingMessages, expandedToolItems, isLoading, loadingNow, loadingStartedAt, objectUrlForRef, toggleThinkingMessage, toggleToolItem]);

  const resolvedHistoryPageSize = Math.max(
    1,
    Math.trunc(historyPageSize || historyMessageLimit || DEFAULT_HISTORY_PAGE_SIZE),
  );

  const getSessionMessagesPage = useCallback(
    async (target: { ns: string; agent: string; sessionId: string }, beforeMessageId?: string | null) => {
      const sessions = gatewayClient?.sessions;
      if (sessions?.listMessages) {
        return sessions.listMessages({
          ...target,
          pageSize: resolvedHistoryPageSize,
          beforeMessageId: beforeMessageId || undefined,
        });
      }

      throw new Error("TalonSession requires a Talon clientset with sessions.listMessages().");
    },
    [gatewayClient, resolvedHistoryPageSize],
  );

  const createSession = useCallback(
    async (target: { ns: string; agent: string }) => {
      const sessions = gatewayClient?.sessions;
      if (sessions?.create) {
        return sessions.create(target);
      }

      throw new Error("TalonSession requires a Talon clientset with sessions.create().");
    },
    [gatewayClient],
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
        const sessions = gatewayClient?.sessions;
        if (!sessions?.streamParts) {
          throw new Error("TalonSession requires a Talon clientset with sessions.streamParts().");
        }
        await streamSessionPartEvents({
          events: sessions.streamParts(target, { signal }),
          setMessages,
          setStreamEvents,
          setError,
          signal,
        });
      } catch (err) {
        if (!signal?.aborted) {
          setError(err instanceof Error ? err : new Error(String(err)));
        }
      }
    },
    [gatewayClient],
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

  const clearLocalSession = useCallback(() => {
    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    setMessages(emptyMessages);
    messagesRef.current = emptyMessages;
    setStreamEvents([]);
    setError(null);
    setHasMoreHistory(false);
    setNextBeforeMessageId(null);
    setIsLoading(false);
    setLoadingStartedAt(null);
    setExpandedThinkingMessages({});
    setExpandedToolItems({});
  }, []);

  const clearSession = useCallback(async () => {
    const session = currentSessionRef.current;
    if (session) {
      const sessions = gatewayClient?.sessions;
      if (sessions?.clear) {
        await sessions.clear(session);
      } else {
        throw new Error("TalonSession requires a Talon clientset with sessions.clear().");
      }
    }
    clearLocalSession();
  }, [clearLocalSession, gatewayClient]);

  const resolvedCommands = useMemo<Array<TalonSessionCommand>>(() => {
    const builtInCommands: TalonSessionCommand[] = [];
    if (enabledBuiltInCommands?.includes("clear")) {
      builtInCommands.push({
        name: "clear",
        description: "Clear the current session history.",
        run: ({ clear }) => clear?.(),
      });
    }
    return [...(commands ?? []), ...builtInCommands];
  }, [clearSession, commands, enabledBuiltInCommands]);
  const commandMenuItems = useMemo(
    () => resolvedCommands.map(({ name, aliases, description }) => ({ name, aliases, description })),
    [resolvedCommands],
  );
  const imageAccept = useMemo(() => acceptedImageTypes.join(","), [acceptedImageTypes]);
  const acceptedImageTypesSet = useMemo(() => new Set(acceptedImageTypes), [acceptedImageTypes]);

  const removeImageAttachment = useCallback((id: string) => {
    setImageAttachments((current) => {
      const removed = current.find((attachment) => attachment.id === id);
      if (removed) {
        URL.revokeObjectURL(removed.previewUrl);
      }
      return current.filter((attachment) => attachment.id !== id);
    });
  }, []);

  const addImageFiles = useCallback((files: File[]) => {
    if (!onImageUpload) return;
    setError(null);
    setImageAttachments((current) => {
      const availableSlots = Math.max(0, maxImageAttachments - current.length);
      const next = [...current];
      for (const file of files.slice(0, availableSlots)) {
        if (!acceptedImageTypesSet.has(file.type)) {
          next.push({
            id: createLocalMessageId(),
            file,
            previewUrl: URL.createObjectURL(file),
            status: "error",
            error: `Unsupported image type: ${file.type || "unknown"}`,
          });
          continue;
        }
        if (file.size > maxImageBytes) {
          next.push({
            id: createLocalMessageId(),
            file,
            previewUrl: URL.createObjectURL(file),
            status: "error",
            error: `Image is larger than ${Math.round(maxImageBytes / (1024 * 1024))} MB`,
          });
          continue;
        }
        next.push({
          id: createLocalMessageId(),
          file,
          previewUrl: URL.createObjectURL(file),
          status: "queued",
        });
      }
      if (files.length > availableSlots) {
        setError(new Error(`You can attach up to ${maxImageAttachments} images.`));
      }
      return next;
    });
  }, [acceptedImageTypesSet, maxImageAttachments, maxImageBytes, onImageUpload]);

  const uploadQueuedImages = useCallback(async (
    session: { ns: string; agent: string; sessionId: string },
    signal: AbortSignal,
  ) => {
    if (!onImageUpload) return imageAttachmentsRef.current;

    const attachments = imageAttachmentsRef.current;
    const failed = attachments.find((attachment) => attachment.status === "error");
    if (failed) {
      throw new Error(failed.error || `Failed to attach ${failed.file.name}`);
    }

    const pendingUploads = attachments.filter((attachment) => !attachment.object);
    if (pendingUploads.length === 0) {
      return attachments;
    }

    const pendingIds = new Set(pendingUploads.map((attachment) => attachment.id));
    const uploadingAttachments = imageAttachmentsRef.current.map((item) =>
      pendingIds.has(item.id) ? { ...item, status: "uploading" as const, error: undefined } : item,
    );
    imageAttachmentsRef.current = uploadingAttachments;
    setImageAttachments(uploadingAttachments);

    const settled = await Promise.allSettled(pendingUploads.map(async (attachment) => ({
      id: attachment.id,
      object: normalizeImageUploadResult(await onImageUpload({
        file: attachment.file,
        namespace: session.ns,
        agent: session.agent,
        sessionId: session.sessionId,
        signal,
      })),
    })));

    const resultsById = new Map<string, { object?: TalonChatObjectRef; error?: string }>();
    settled.forEach((result, index) => {
      const attachment = pendingUploads[index];
      if (!attachment) return;
      if (result.status === "fulfilled") {
        resultsById.set(attachment.id, { object: result.value.object });
      } else {
        const reason = result.reason;
        resultsById.set(attachment.id, {
          error: reason instanceof Error ? reason.message : String(reason || `Failed to attach ${attachment.file.name}`),
        });
      }
    });

    const nextAttachments = imageAttachmentsRef.current.map((item) => {
      const result = resultsById.get(item.id);
      if (!result) return item;
      return result.object
        ? { ...item, object: result.object, status: "ready" as const, error: undefined }
        : { ...item, status: "error" as const, error: result.error || `Failed to attach ${item.file.name}` };
    });
    imageAttachmentsRef.current = nextAttachments;
    setImageAttachments(nextAttachments);

    const uploadFailure = nextAttachments.find((attachment) => attachment.status === "error");
    if (uploadFailure) {
      throw new Error(uploadFailure.error || `Failed to attach ${uploadFailure.file.name}`);
    }

    return nextAttachments;
  }, [onImageUpload]);

  const submitMessage = useCallback(async (submittedText: string) => {
    const text = submittedText.trim();
    const pendingAttachments = imageAttachmentsRef.current;
    const hasImages = pendingAttachments.length > 0;
    if ((!text && !hasImages) || isLoading || disabled) return;
    let submitTurnStarted = false;

    const parsedCommand = parseTalonChatCommandInput(text);
    const command = findTalonChatCommand(resolvedCommands, parsedCommand);
    if (command && parsedCommand && !hasImages) {
      setInput("");
      setError(null);
      setStreamEvents([]);
      try {
        await command.run({
          name: parsedCommand.name,
          input: text,
          args: parsedCommand.args,
          argv: parsedCommand.argv,
          target: {
            type: "session",
            namespace,
            agent,
            sessionId: currentSessionRef.current?.sessionId ?? sessionId ?? null,
          },
          messages: messagesRef.current,
          clear: clearSession,
        });
      } catch (err) {
        setError(err instanceof Error ? err : new Error(String(err)));
      }
      return;
    }

    setError(null);
    setStreamEvents([]);
    resumeAbortControllerRef.current?.abort();
    resumeAbortControllerRef.current = null;

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

      const controller = new AbortController();
      abortControllerRef.current = controller;
      const uploadedImages = await uploadQueuedImages(session, controller.signal);
      const imageParts = uploadedImages.map((attachment) => {
        if (!attachment.object) {
          throw new Error(`Image ${attachment.file.name} was not uploaded.`);
        }
        return {
          type: "image",
          object: normalizeObjectRefForJson({
            ...attachment.object,
            filename: attachment.object.filename || attachment.file.name,
            mediaType: objectRefMediaType(attachment.object) || attachment.file.type,
            sizeBytes: attachment.object.sizeBytes ?? attachment.object.size_bytes ?? attachment.file.size,
          }),
          previewUrl: attachment.previewUrl,
          payloadJson: JSON.stringify({ filename: attachment.file.name }),
        };
      });
      const messageParts = [
        ...(text ? [{ type: "text", text }] : []),
        ...imageParts,
      ];
      const userMessage: CopilotMessage = {
        id: createLocalMessageId(),
        role: "user",
        content: text,
        parts: messageParts,
        createdAt: String(Date.now() * 1000),
      };

      setInput("");
      submittedPreviewUrlsRef.current.push(...uploadedImages.map((attachment) => attachment.previewUrl));
      setImageAttachments([]);
      setMessages((prev) => [...prev, userMessage]);
      setLoadingStartedAt(normalizeEpochToMilliseconds(userMessage.createdAt) ?? Date.now());
      setLoadingNow(Date.now());
      setIsLoading(true);

      const sessions = gatewayClient?.sessions;
      if (!sessions?.submitTurn) {
        throw new Error("TalonSession requires a Talon clientset with sessions.submitTurn().");
      }

      submitTurnStarted = true;
      const { assistantText } = await streamSessionPartEvents({
        events: sessions.submitTurn({
          ns: session.ns,
          agent: session.agent,
          sessionId: session.sessionId,
          message: {
            role: 1,
            parts: protoSessionPartsFromChatParts(userMessage.parts),
          },
          labels: {},
        }, { signal: controller.signal }),
        setMessages,
        setStreamEvents,
        setError,
        signal: controller.signal,
      });

      if (!assistantText) {
        await waitForCanonicalAssistantUpdate(session, baselineAssistantSignature);
      } else {
        await refreshNewestSessionPage(session);
      }
    } catch (err: any) {
      const nextError = err instanceof Error ? err : new Error(String(err));
      const session = currentSessionRef.current;
      if (session && submitTurnStarted) {
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
  }, [agent, clearSession, createSession, disabled, gatewayClient, isLoading, namespace, onSessionChange, refreshNewestSessionPage, resolvedCommands, resolvedHistoryPageSize, sessionId, uploadQueuedImages, waitForCanonicalAssistantUpdate]);

  const stopGeneration = useCallback(async () => {
    if (!currentSessionRef.current || !isLoading) return;

    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    setIsLoading(false);
    setLoadingStartedAt(null);

    const session = currentSessionRef.current;
    const sessions = gatewayClient?.sessions;
    if (!sessions?.stopGeneration) {
      throw new Error("TalonSession requires a Talon clientset with sessions.stopGeneration().");
    }
    await sessions.stopGeneration(session);
  }, [gatewayClient, isLoading]);

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
              canSubmit={Boolean((input || "").trim() || imageAttachments.length > 0) && !isLoading}
              isGenerating={isLoading}
              canStop={Boolean(currentSession)}
              commandMenuItems={commandMenuItems}
              imageAttachments={imageAttachments.map((attachment) => ({
                id: attachment.id,
                filename: attachment.file.name,
                previewUrl: attachment.previewUrl,
                status: attachment.status,
                error: attachment.error,
              }))}
              imageUploadEnabled={Boolean(onImageUpload)}
              imageAccept={imageAccept}
              onImageFilesSelected={addImageFiles}
              onRemoveImageAttachment={removeImageAttachment}
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
