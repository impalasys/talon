"use client";

import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { data, type TalonClient } from "@impalasys/talon-client";
import { decompress as decompressZstd } from "fzstd";
import { Activity, Check, ChevronRight, Copy, Pencil, X, Wrench } from "lucide-react";
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
import { TalonChatComposer, type TalonChatComposerVariant } from "./lib/TalonChatComposer";
import {
  findTalonChatCommand,
  parseTalonChatCommandInput,
  type TalonBuiltInCommandName,
  type TalonChatCommand,
} from "./lib/commands";
import { MarkdownMessage } from "./lib/MarkdownMessage";
import { ResourcePane } from "./lib/ResourcePane";
import {
  parseResourceUri,
  type ResourceViewModel,
} from "./lib/resourceUris";
import { streamSessionPartEvents, type StreamEventItem } from "./lib/uiStream";
import {
  SESSION_MESSAGE_PART_TYPE,
  messagePartsForSessionUpdate,
  parsePayloadJson,
  protoSessionPartsFromChatParts,
  isSessionTextPart,
} from "./session/protocol";
import {
  normalizeImageUploadResult,
  normalizeObjectRefForJson,
  objectRefContentEncoding,
  objectRefFromPart,
  objectRefFromValue,
  objectRefKey,
  objectRefMediaType,
  objectRefSizeBytes,
} from "./session/objectRefs";
import type { TalonChatObjectRef, TalonSessionHandle } from "./session/types";
import { useSessionRuntime } from "./session/useSessionRuntime";
import type { SessionTarget } from "./session/types";
import { SessionTranscript } from "./session/SessionTranscript";
import { SessionComposerDock } from "./session/SessionComposerDock";
import {
  canCompareCanonicalMessageIds,
  historyMessageTimestamp,
  isLocalMessageId,
  mergeNewestCanonicalPage,
  normalizeHistoryPage,
  normalizeMessageLabels,
  normalizeRawSessionMessage,
  type SessionHistoryPage,
} from "./session/history";

const useSafeLayoutEffect = typeof window !== "undefined" ? useLayoutEffect : useEffect;

export type SessionServiceClientLike = {
  sessions: Pick<
    TalonClient["sessions"],
    "create" | "clear" | "listMessages" | "submitTurn" | "streamParts" | "stopGeneration"
  > & Partial<Pick<TalonClient["sessions"], "appendMessage" | "updateMessage">>;
}["sessions"];

export type CasServiceClientLike = Pick<TalonClient["cas"], "getObject">;

export type ArtifactServiceClientLike = Pick<
  TalonClient["artifacts"],
  "readArtifact" | "getArtifactMetadata"
>;

export type FileServiceClientLike = Pick<
  TalonClient["files"],
  "readFile" | "getFileMetadata"
>;

export type GatewayClientLike = {
  sessions: SessionServiceClientLike;
  cas?: CasServiceClientLike;
  artifacts?: ArtifactServiceClientLike;
  files?: FileServiceClientLike;
};

export type { ResourceViewModel };

export type TalonSessionCommandTarget = {
  type: "session";
  namespace: string;
  agent: string;
  sessionId: string | null;
};

export type TalonSessionCommand = TalonChatCommand<TalonSessionCommandTarget, CopilotMessage>;

export type { TalonChatObjectRef } from "./session/types";

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

export type TalonSessionPendingImageAttachment = {
  id: string;
  file: File;
  previewUrl: string;
  object?: TalonChatObjectRef;
  status: "queued" | "uploading" | "ready" | "error";
  error?: string;
};

export type TalonSessionSubmitContext = {
  text: string;
  namespace: string;
  agent: string;
  sessionId: string | null;
  imageAttachments: ReadonlyArray<TalonSessionPendingImageAttachment>;
  ensureSession: () => Promise<TalonSessionHandle>;
  clearInput: () => void;
  refreshSession: () => Promise<void>;
};

export type TalonSessionMessageEditContext = {
  message: CopilotMessage;
  nextContent: string;
  namespace: string;
  agent: string;
  sessionId: string | null;
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
  composerVariant?: TalonChatComposerVariant;
  composerStartAdornment?: React.ReactNode;
  composerEndAdornment?: React.ReactNode;
  onSubmitMessage?: (context: TalonSessionSubmitContext) => Promise<boolean | void> | boolean | void;
  allowMessageEditing?: boolean;
  onMessageEdit?: (context: TalonSessionMessageEditContext) => Promise<boolean | void> | boolean | void;
  enableDebugMessageEditing?: boolean;
  /**
   * Called when an artifact:// or file:// link is clicked.
   * If omitted, the built-in split pane opens when the matching client is available.
   */
  onResourceClick?: (uri: string) => void;
  /**
   * Override content fetch for the built-in resource pane (both kinds).
   */
  fetchResource?: (uri: string, signal: AbortSignal) => Promise<ResourceViewModel>;
};

export type TalonCopilotProps = TalonSessionProps;

const emptyMessages: CopilotMessage[] = [];
const DEFAULT_HISTORY_PAGE_SIZE = 50;
const DEFAULT_HISTORY_MESSAGE_LIMIT = 100;
const DEFAULT_HISTORY_STEP_LIMIT = 1000;
const HISTORY_SCROLL_LOAD_THRESHOLD_PX = 120;
const AUTO_SCROLL_BOTTOM_THRESHOLD_PX = 48;
const LABEL_CONNECTOR_DELIVERY_STATUS = "talon.impalasys.com/connector-delivery-status";
const LABEL_CONNECTOR_DELIVERY_ERROR = "talon.impalasys.com/connector-delivery-error";
const CONNECTOR_DELIVERY_PENDING_REVIEW = "pending_review";
const CONNECTOR_DELIVERY_REQUESTED = "delivery_requested";
const CONNECTOR_DELIVERY_SKIPPED = "skipped";

function border(color: string) {
  return `1px solid ${color}`;
}

const HANDLE_CALLER_AGENT_HEADER = "x-talon-agent";
const HANDLE_CALLER_SESSION_HEADER = "x-talon-session-id";

function resourceCallHeaders(agent: string, sessionId: string | null | undefined) {
  const headers: Record<string, string> = {};
  if (agent) headers[HANDLE_CALLER_AGENT_HEADER] = agent;
  if (sessionId) headers[HANDLE_CALLER_SESSION_HEADER] = sessionId;
  return Object.keys(headers).length > 0 ? headers : undefined;
}

/** Strict base64 shape: length multiple of 4 and only base64 alphabet + padding. */
const BASE64_CONTENT_RE = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;

function looksLikeBase64(value: string): boolean {
  if (value.length === 0 || value.length % 4 !== 0) return false;
  // Avoid treating short plain words as base64 (e.g. "test" is valid alphabet).
  if (value.length < 16) return false;
  return BASE64_CONTENT_RE.test(value);
}

function bytesFromContent(content: unknown): Uint8Array | undefined {
  if (content == null) return undefined;
  if (content instanceof Uint8Array) return content;
  if (typeof content === "string") {
    if (content.length === 0) return new Uint8Array(0);
    // Connect JSON may base64-encode bytes. Only decode when the string looks like
    // base64; otherwise treat as literal UTF-8 text (mocks and plain responses).
    if (looksLikeBase64(content)) {
      try {
        const binary = atob(content);
        const bytes = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
        return bytes;
      } catch {
        // Fall through to UTF-8 if padded base64 shape still fails to decode.
      }
    }
    return new TextEncoder().encode(content);
  }
  if (ArrayBuffer.isView(content)) {
    return new Uint8Array(content.buffer, content.byteOffset, content.byteLength);
  }
  return undefined;
}

async function fetchResourceFromGateway(options: {
  uri: string;
  gatewayClient: GatewayClientLike;
  agent: string;
  sessionId: string | null;
  signal: AbortSignal;
}): Promise<ResourceViewModel> {
  const { uri, gatewayClient, agent, sessionId, signal } = options;
  const parsed = parseResourceUri(uri);
  if (!parsed) {
    throw new Error(`Unsupported resource URI: ${uri}`);
  }

  const headers = resourceCallHeaders(agent, sessionId);
  const callOptions = {
    signal,
    ...(headers ? { headers } : {}),
  };

  if (parsed.kind === "artifact") {
    const artifacts = gatewayClient.artifacts;
    if (!artifacts?.readArtifact) {
      throw new Error("Gateway client does not expose artifacts.readArtifact().");
    }
    const response = await (artifacts.readArtifact as any)(
      { artifactUri: parsed.uri },
      callOptions,
    );
    const artifact = response?.artifact ?? {};
    const mediaType =
      artifact.mediaType || artifact.media_type || "application/octet-stream";
    const title =
      (typeof artifact.title === "string" && artifact.title) || parsed.artifactId;
    return {
      kind: "artifact",
      uri: parsed.uri,
      title,
      mediaType,
      content: bytesFromContent(response?.content),
      signedUrl: response?.signedUrl || response?.signed_url || undefined,
      sessionId: parsed.sessionId,
      agent: parsed.agent,
    };
  }

  const files = gatewayClient.files;
  if (!files?.readFile) {
    throw new Error("Gateway client does not expose files.readFile().");
  }
  const response = await (files.readFile as any)(
    { file: { uri: parsed.uri } },
    callOptions,
  );
  const file = response?.file ?? {};
  const meta = file.metadata ?? {};
  const spec = file.spec ?? {};
  const mediaType =
    spec.mediaType ||
    spec.media_type ||
    "application/octet-stream";
  const title =
    (typeof meta.name === "string" && meta.name) ||
    (typeof spec.path === "string" && spec.path.split("/").filter(Boolean).pop()) ||
    parsed.fileName;
  return {
    kind: "file",
    uri: parsed.uri,
    title,
    mediaType,
    content: bytesFromContent(response?.content),
    signedUrl: response?.signedUrl || response?.signed_url || undefined,
    path: typeof spec.path === "string" ? spec.path : undefined,
  };
}

const talonChatFontFamily =
  'var(--talon-chat-font-family, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif)';
const talonChatMessageFontSize = "var(--talon-chat-message-font-size, 1rem)";

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

function sessionProcessingStartTime(messages: CopilotMessage[]) {
  const latestUserMessage = [...messages].reverse().find((message) => message.role === "user");
  return latestUserMessage ? normalizeEpochToMilliseconds(latestUserMessage.createdAt) : null;
}

function getAssistantSignature(messages: any[] | undefined) {
  if (!Array.isArray(messages)) return "";
  return messages
    .filter((message) => message?.role === "assistant" || message?.role === 2 || message?.role === "ROLE_ASSISTANT")
    .map((message) => `${String(message.id ?? "")}:${getMessageContent(message).length}`)
    .join("|");
}

type ScrollThumbState = {
  visible: boolean;
  top: number;
  height: number;
};

function replaceMessageTextPart(message: CopilotMessage, text: string) {
  const sourceParts = Array.isArray(message.parts) ? message.parts : [];
  const parts = sourceParts.map((part: any) => part && typeof part === "object" ? { ...part } : part);
  let textPartIndex = -1;
  for (let index = parts.length - 1; index >= 0; index -= 1) {
    if (isSessionTextPart(parts[index])) {
      textPartIndex = index;
      break;
    }
  }
  if (textPartIndex >= 0) {
    const part = parts[textPartIndex] as any;
    if ("text" in part) {
      part.text = text;
    } else {
      part.content = text;
    }
    return parts;
  }
  return [
    ...parts,
    {
      partType: SESSION_MESSAGE_PART_TYPE.TEXT,
      content: text,
    },
  ];
}

function isToolResultPart(part: any) {
  const type = part?.type ?? part?.partType ?? part?.part_type;
  return type === SESSION_MESSAGE_PART_TYPE.TOOL_RESULT || type === "SESSION_MESSAGE_PART_TYPE_TOOL_RESULT";
}

function withToolResultContent(part: any, output: string, objectKey?: string) {
  const nextPart = { ...part, content: output };
  const payload = parsePayloadJson(part?.payloadJson ?? part?.payload_json);
  const nextPayload = { ...payload, output };
  const toolOutputKey = "tool_output" in payload ? "tool_output" : "toolOutput";
  const toolOutput = payload[toolOutputKey];
  const toolOutputRecord = toolOutput && typeof toolOutput === "object"
    ? toolOutput as Record<string, unknown>
    : undefined;
  if (toolOutputRecord) {
    const outputPartsKey = "content_parts" in toolOutputRecord ? "content_parts" : "contentParts";
    const outputParts = toolOutputRecord[outputPartsKey];
    if (Array.isArray(outputParts)) {
      nextPayload[toolOutputKey] = {
        ...toolOutputRecord,
        summary: output,
        [outputPartsKey]: outputParts.map((outputPart) => {
          const object = objectRefFromValue(outputPart);
          return object && (!objectKey || object.key === objectKey)
            ? { type: "text", text: output }
            : outputPart;
        }),
      };
    }
  }
  if ("payload_json" in nextPart && !("payloadJson" in nextPart)) {
    nextPart.payload_json = JSON.stringify(nextPayload);
  } else {
    nextPart.payloadJson = JSON.stringify(nextPayload);
  }
  return nextPart;
}

async function decompressCasObjectData(data: Uint8Array, encoding: string): Promise<Uint8Array> {
  if (typeof DecompressionStream === "undefined") {
    throw new Error(`${encoding} CAS object requires DecompressionStream support`);
  }
  const bytes = new ArrayBuffer(data.byteLength);
  new Uint8Array(bytes).set(data);
  const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream(encoding as any));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

async function decompressZstdCasObjectData(data: Uint8Array): Promise<Uint8Array> {
  if (typeof DecompressionStream !== "undefined") {
    try {
      return await decompressCasObjectData(data, "zstd");
    } catch (err) {
      if (!(err instanceof TypeError)) {
        throw err;
      }
    }
  }
  return decompressZstd(data);
}

async function casObjectData(response: any): Promise<Uint8Array> {
  const signedUrl = typeof response?.signedUrl === "string"
    ? response.signedUrl
    : typeof response?.signed_url === "string"
      ? response.signed_url
      : "";
  if (signedUrl) {
    const fetched = await fetch(signedUrl);
    if (!fetched.ok) {
      throw new Error(`Failed to fetch CAS object: HTTP ${fetched.status}`);
    }
    return new Uint8Array(await fetched.arrayBuffer());
  }
  return response.data ?? new Uint8Array();
}

async function toolResultObjectData(response: any, fallbackObject?: TalonChatObjectRef): Promise<Uint8Array> {
  const bytes = await casObjectData(response);
  const responseEncoding = response?.contentEncoding
    ?? response?.content_encoding
    ?? response?.metadata?.content_encoding
    ?? response?.metadata?.contentEncoding;
  const encoding = typeof responseEncoding === "string" ? responseEncoding : objectRefContentEncoding(fallbackObject);
  const normalized = encoding.toLowerCase();
  if (normalized === "zstd") return decompressZstdCasObjectData(bytes);
  if (normalized === "gzip") return decompressCasObjectData(bytes, normalized);
  return bytes;
}

function toolCallIdFromToolResultPart(part: any): string {
  if (typeof part?.toolCallId === "string") return part.toolCallId;
  if (typeof part?.tool_call_id === "string") return part.tool_call_id;
  const payload = parsePayloadJson(part?.payloadJson ?? part?.payload_json);
  const payloadToolCallId = payload.tool_call_id ?? payload.toolCallId;
  if (typeof payloadToolCallId === "string") return payloadToolCallId;
  return typeof part?.id === "string" ? part.id : "";
}

function findHydratableToolResultPart(
  parts: unknown,
  toolCallId: string,
): { part: any; index: number; key: string; object: TalonChatObjectRef } | null {
  if (!Array.isArray(parts)) return null;
  for (let index = 0; index < parts.length; index += 1) {
    const part = parts[index] as any;
    if (!part || typeof part !== "object" || !isToolResultPart(part)) continue;
    if (typeof part.content === "string" && part.content.length > 0) continue;
    const key = objectRefKey(objectRefFromPart(part));
    if (!key) continue;
    const partToolCallId = toolCallIdFromToolResultPart(part);
    if (toolCallId && partToolCallId !== toolCallId) continue;
    const object = objectRefFromPart(part);
    if (!object) continue;
    return { part, index, key, object };
  }
  return null;
}

function messageImageParts(
  message: CopilotMessage,
  objectUrlForRef?: (object: TalonChatObjectRef) => string | undefined,
): Array<{ id: string; src?: string; label: string }> {
  if (!Array.isArray(message.parts)) return [];
  return message.parts.flatMap((part: any, index) => {
    const type = part?.type ?? part?.partType ?? part?.part_type;
    if (type !== "image" && type !== SESSION_MESSAGE_PART_TYPE.IMAGE && type !== "SESSION_MESSAGE_PART_TYPE_IMAGE") {
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

function coalesceAssistantTimelineForDisplay(timeline: AssistantTimelineItem[]) {
  const nextTimeline: AssistantTimelineItem[] = [];
  let latestUsage: Extract<AssistantTimelineItem, { type: "usage" }> | null = null;

  for (const item of timeline) {
    if (item.type === "usage") {
      latestUsage = item;
      continue;
    }

    if (item.type === "text") {
      const lastItem = nextTimeline.at(-1);
      if (lastItem?.type === "text") {
        nextTimeline[nextTimeline.length - 1] = {
          type: "text",
          text: `${lastItem.text}${item.text}`,
        };
      } else {
        nextTimeline.push(item);
      }
      continue;
    }

    if (item.type === "reasoning") {
      const lastItem = nextTimeline.at(-1);
      if (lastItem?.type === "reasoning") {
        nextTimeline[nextTimeline.length - 1] = {
          type: "reasoning",
          text: `${lastItem.text}${item.text}`,
        };
      } else {
        nextTimeline.push(item);
      }
      continue;
    }

    nextTimeline.push(item);
  }

  if (latestUsage) {
    nextTimeline.push(latestUsage);
  }

  return nextTimeline;
}

function ContextCompactionDivider({ compacting }: { compacting: boolean }) {
  const label = compacting ? "Context compacting automatically" : "Context compacted";
  return (
    <div
      className="talon-session-compaction-divider"
      role="separator"
      aria-label={label}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        width: "100%",
        color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))",
        fontSize: 12,
        fontWeight: 500,
        letterSpacing: "0.01em",
        padding: "0.5rem 0",
      }}
    >
      <span aria-hidden="true" style={{ flex: 1, borderTop: border("var(--talon-chat-divider, rgba(212,212,216,0.7))") }} />
      <span>{label}</span>
      <span aria-hidden="true" style={{ flex: 1, borderTop: border("var(--talon-chat-divider, rgba(212,212,216,0.7))") }} />
    </div>
  );
}

function splitFinalAssistantTimeline(timeline: AssistantTimelineItem[]) {
  const displayTimeline = coalesceAssistantTimelineForDisplay(timeline);
  let finalTextIndex = -1;
  for (let index = displayTimeline.length - 1; index >= 0; index -= 1) {
    const item = displayTimeline[index];
    if (item?.type === "text" && item.text.trim().length > 0) {
      finalTextIndex = index;
      break;
    }
  }
  if (finalTextIndex < 0) {
    return { workTimeline: displayTimeline, finalTimeline: [] };
  }
  return {
    workTimeline: displayTimeline.filter((_, index) => index !== finalTextIndex),
    finalTimeline: [displayTimeline[finalTextIndex]],
  };
}

function isNearScrollBottom(container: HTMLElement) {
  return container.scrollHeight - container.scrollTop - container.clientHeight <= AUTO_SCROLL_BOTTOM_THRESHOLD_PX;
}

function formatMessageActionTimestamp(message: CopilotMessage) {
  const timestampMs = historyMessageTimestamp(message);
  if (timestampMs === null) {
    return null;
  }
  return new Date(timestampMs).toLocaleTimeString([], {
    hour: "numeric",
    minute: "2-digit",
  });
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

function isSessionBusyError(error: unknown) {
  const candidate = error as { message?: unknown } | null;
  return typeof candidate?.message === "string" && /session is currently generating|session is busy/i.test(candidate.message);
}

function messageWithEditedContent(message: CopilotMessage, nextContent: string): CopilotMessage {
  const nextMessage: CopilotMessage = { ...message, content: nextContent };
  if (Array.isArray(nextMessage.parts)) {
    let replacedTextPart = false;
    nextMessage.parts = nextMessage.parts.map((part: any) => {
      if (!part || typeof part !== "object") {
        return part;
      }
      const type = part?.partType ?? part?.part_type ?? part?.type;
      const isTextPart =
        type === "text" ||
        type === SESSION_MESSAGE_PART_TYPE.TEXT ||
        type === "SESSION_MESSAGE_PART_TYPE_TEXT";
      if (!isTextPart || replacedTextPart) {
        return part;
      }
      replacedTextPart = true;
      const nextPart = { ...part };
      if ("text" in nextPart) nextPart.text = nextContent;
      if ("content" in nextPart) nextPart.content = nextContent;
      return nextPart;
    });
  }
  if (message.role === "assistant") {
    nextMessage.timeline = [{ type: "text", text: nextContent }];
  }
  return nextMessage;
}

function editableMessageContent(message: CopilotMessage) {
  if (message.role === "assistant") {
    const timeline = coalesceAssistantTimelineForDisplay(getMessageAssistantTimeline(message));
    const { finalTimeline } = splitFinalAssistantTimeline(timeline);
    const visibleTextTimeline = finalTimeline.length > 0 ? finalTimeline : timeline;
    const textItems = visibleTextTimeline
      .filter((item): item is Extract<AssistantTimelineItem, { type: "text" }> => item.type === "text");
    if (textItems.length > 0) {
      return textItems.map((item) => item.text).join("");
    }
  }
  return getMessageContent(message);
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
  composerVariant = "panel",
  composerStartAdornment,
  composerEndAdornment,
  onSubmitMessage,
  allowMessageEditing = false,
  onMessageEdit,
  enableDebugMessageEditing = false,
  onResourceClick: onResourceClickProp,
  fetchResource,
}: TalonSessionProps) {
  const initialSessionTarget = useMemo<SessionTarget | null>(
    () => sessionId ? { ns: namespace, agent, sessionId } : null,
    [agent, namespace, sessionId],
  );
  const runtimeClient = useMemo(() => ({
    listMessages: async (target: SessionTarget, options?: { beforeMessageId?: string | null; pageSize?: number; signal?: AbortSignal }) => {
      const response = await gatewayClient.sessions.listMessages({
        ...target,
        pageSize: options?.pageSize,
        beforeMessageId: options?.beforeMessageId || undefined,
      });
      return normalizeHistoryPage(response);
    },
  }), [gatewayClient]);
  const sessionRuntime = useSessionRuntime({
    target: initialSessionTarget,
    client: runtimeClient,
    pageSize: Math.max(1, Math.trunc(historyPageSize || historyMessageLimit || DEFAULT_HISTORY_PAGE_SIZE)),
  });
  const {
    state: sessionRuntimeState,
    isLive: runtimeIsLive,
    setMessages,
    setPhase,
    setServerState,
    setError,
    refresh: refreshRuntime,
    loadOlder: loadOlderRuntime,
    clear: clearRuntime,
    activateTarget,
  } = sessionRuntime;
  const messages = sessionRuntimeState.messages;
  const currentSession = sessionRuntimeState.target;
  const isLoading = sessionRuntimeState.phase === "submitting";
  const isResuming = sessionRuntimeState.phase === "resuming";
  const isStopping = sessionRuntimeState.phase === "stopping";
  const sessionState = sessionRuntimeState.serverState === "UNKNOWN" ? null : sessionRuntimeState.serverState;
  const isSessionLive = runtimeIsLive;
  const setIsLoading = useCallback((value: boolean) => setPhase(value ? "submitting" : "idle"), [setPhase]);
  const setIsResuming = useCallback((value: boolean) => setPhase(value ? "resuming" : "idle"), [setPhase]);
  const setIsStopping = useCallback((value: boolean) => setPhase(value ? "stopping" : "idle"), [setPhase]);
  const setSessionState = useCallback((value: string | null) => {
    setServerState(value === "PROCESSING" ? "PROCESSING" : value === "ERROR" ? "ERROR" : value ? "IDLE" : "UNKNOWN");
  }, [setServerState]);
  const [input, setInput] = useState("");
  const [imageAttachments, setImageAttachments] = useState<TalonSessionPendingImageAttachment[]>([]);
  const [loadingStartedAt, setLoadingStartedAt] = useState<string | number | null>(null);
  const [loadingNow, setLoadingNow] = useState(Date.now());
  const error = sessionRuntimeState.error;
  const [streamEvents, setStreamEvents] = useState<StreamEventItem[]>([]);
  const [expandedThinkingMessages, setExpandedThinkingMessages] = useState<Record<string, boolean>>({});
  const [expandedToolItems, setExpandedToolItems] = useState<Record<string, boolean>>({});
  const [toolResultHydration, setToolResultHydration] = useState<Record<string, "loading" | "error">>({});
  const [openResourceUri, setOpenResourceUri] = useState<string | null>(null);
  /** False while the pane plays its close transition before unmount. */
  const [resourcePaneOpen, setResourcePaneOpen] = useState(false);
  const [resourceView, setResourceView] = useState<ResourceViewModel | null>(null);
  const [resourceLoading, setResourceLoading] = useState(false);
  const [resourceError, setResourceError] = useState<Error | null>(null);
  const resourceAbortRef = useRef<AbortController | null>(null);
  const missingResourceClientWarnedRef = useRef(false);
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [editingMessageValue, setEditingMessageValue] = useState("");
  const [reviewActionMessageId, setReviewActionMessageId] = useState<string | null>(null);
  const hasMoreHistory = sessionRuntimeState.history.hasMoreOlder;
  const nextBeforeMessageId = sessionRuntimeState.history.beforeMessageId;
  const [isLoadingOlderHistory, setIsLoadingOlderHistory] = useState(false);
  const [scrollThumb, setScrollThumb] = useState<ScrollThumbState>({ visible: false, top: 0, height: 0 });
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const abortControllerRef = useRef<AbortController | null>(null);
  const resumeAbortControllerRef = useRef<AbortController | null>(null);
  const stopAbortControllerRef = useRef<AbortController | null>(null);
  const currentSessionRef = useRef<SessionTarget | null>(null);
  const messagesRef = useRef<CopilotMessage[]>(emptyMessages);
  const imageAttachmentsRef = useRef<TalonSessionPendingImageAttachment[]>([]);
  const submittedPreviewUrlsRef = useRef<string[]>([]);
  const toolResultHydrationInFlightRef = useRef<Set<string>>(new Set());
  const toolResultHydrationGenerationRef = useRef(0);
  const isStoppingRef = useRef(false);
  const skipNextAutoScrollRef = useRef(false);
  const autoScrollPinnedRef = useRef(true);
  const prependScrollRestoreRef = useRef<{ previousScrollTop: number; previousScrollHeight: number } | null>(null);
  const isLoadingOlderHistoryRef = useRef(false);

  const invalidateToolResultHydration = useCallback(() => {
    toolResultHydrationGenerationRef.current += 1;
    toolResultHydrationInFlightRef.current.clear();
    setToolResultHydration({});
  }, []);

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

  const previousSessionTargetRef = useRef<SessionTarget | null>(null);
  useEffect(() => {
    const previousTarget = previousSessionTargetRef.current;
    previousSessionTargetRef.current = currentSession;
    if (!previousTarget || !currentSession || isSameSession(previousTarget, currentSession)) return;
    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    resumeAbortControllerRef.current?.abort();
    resumeAbortControllerRef.current = null;
    stopAbortControllerRef.current?.abort();
    stopAbortControllerRef.current = null;
    isStoppingRef.current = false;
    setIsStopping(false);
    setIsLoading(false);
    setIsResuming(false);
    setLoadingStartedAt(null);
    setStreamEvents([]);
    setExpandedThinkingMessages({});
    invalidateToolResultHydration();
  }, [currentSession?.agent, currentSession?.ns, currentSession?.sessionId, invalidateToolResultHydration, setIsLoading, setIsResuming, setIsStopping]);

  const scrollTranscriptToBottom = useCallback((behavior: ScrollBehavior) => {
    autoScrollPinnedRef.current = true;
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
      if (autoScrollPinnedRef.current) {
        scrollTranscriptToBottom("auto");
      }
      updateTranscriptScrollThumb();
    });
    return () => window.cancelAnimationFrame(rafId);
  }, [currentSession?.sessionId, messages, streamEvents, isSessionLive, error, scrollTranscriptToBottom, updateTranscriptScrollThumb]);

  useEffect(() => {
    updateTranscriptScrollThumb();
    window.addEventListener("resize", updateTranscriptScrollThumb);
    return () => window.removeEventListener("resize", updateTranscriptScrollThumb);
  }, [updateTranscriptScrollThumb]);

  useSafeLayoutEffect(() => {
    updateTranscriptScrollThumb();
  }, [messages, expandedThinkingMessages, expandedToolItems, toolResultHydration, isSessionLive, error, streamEvents, updateTranscriptScrollThumb]);

  useEffect(() => {
    if (!isSessionLive || loadingStartedAt === null) {
      return;
    }
    setLoadingNow(Date.now());
    const intervalId = window.setInterval(() => setLoadingNow(Date.now()), 250);
    return () => window.clearInterval(intervalId);
  }, [isSessionLive, loadingStartedAt]);

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

  const hydrateToolResultForExpandedItem = useCallback(
    async (message: CopilotMessage, toolCallId: string, toolKey: string, result?: unknown) => {
      const partMatch = findHydratableToolResultPart(message.parts, toolCallId);
      const resultObject = partMatch ? undefined : objectRefFromValue(result);
      const match = partMatch ?? (resultObject
        ? { part: undefined, index: -1, key: resultObject.key, object: resultObject }
        : null);
      const cas = gatewayClient?.cas;
      if (!match || !cas?.getObject) return;
      if (toolResultHydrationInFlightRef.current.has(toolKey)) return;

      toolResultHydrationInFlightRef.current.add(toolKey);
      const generation = toolResultHydrationGenerationRef.current;
      setToolResultHydration((prev) => ({
        ...prev,
        [toolKey]: "loading",
      }));

      try {
        const response = await cas.getObject({ key: match.key });
        const data = await toolResultObjectData(response, match.object);
        const output = new TextDecoder().decode(data);
        if (toolResultHydrationGenerationRef.current !== generation) return;

        setMessages((current) => {
          if (toolResultHydrationGenerationRef.current !== generation) return current;
          const nextMessages = current.map((candidate) => {
            if (candidate.id !== message.id) return candidate;
            const currentMatch = findHydratableToolResultPart(candidate.parts, toolCallId);
            if (currentMatch && currentMatch.key === match.key && Array.isArray(candidate.parts)) {
              const parts = [...candidate.parts];
              parts[currentMatch.index] = withToolResultContent(currentMatch.part, output, match.key);
              const timelineSource = {
                role: candidate.role,
                content: candidate.content,
                parts,
                reasoningContent: candidate.reasoningContent,
                usage: candidate.usage,
              };
              return {
                ...candidate,
                parts,
                content: getMessageContent(timelineSource),
                timeline: getMessageAssistantTimeline(timelineSource),
              };
            }

            const timeline = getMessageAssistantTimeline(candidate).map((item) =>
              item.type === "tool" && item.toolCallId === toolCallId
                ? { ...item, result: output }
                : item,
            );
            return {
              ...candidate,
              timeline,
            };
          });
          messagesRef.current = nextMessages;
          return nextMessages;
        });
        if (toolResultHydrationGenerationRef.current !== generation) return;

        setToolResultHydration((prev) => {
          if (!(toolKey in prev)) return prev;
          const next = { ...prev };
          delete next[toolKey];
          return next;
        });
      } catch (err) {
        if (toolResultHydrationGenerationRef.current !== generation) return;
        console.warn("Could not hydrate CAS tool-result object", match.key, err);
        setToolResultHydration((prev) => ({
          ...prev,
          [toolKey]: "error",
        }));
      } finally {
        toolResultHydrationInFlightRef.current.delete(toolKey);
      }
    },
    [gatewayClient],
  );

  const updateSessionMessage = useCallback(
    async (message: CopilotMessage, parts: unknown[], labels: Record<string, string>) => {
      const session = currentSessionRef.current ?? (sessionId ? { ns: namespace, agent, sessionId } : null);
      const sessions = gatewayClient?.sessions;
      if (!session) {
        throw new Error("No active session to update.");
      }
      if (!sessions?.updateMessage) {
        throw new Error("Gateway client does not support sessions.updateMessage().");
      }
      const response = await sessions.updateMessage({
        ns: session.ns,
        agent: session.agent,
        sessionId: session.sessionId,
        messageId: message.id,
        parts,
        labels,
      });
      const updated = response?.message
        ? { ...message, ...normalizeRawSessionMessage(response.message) }
        : { ...message, parts, labels, content: getMessageContent({ ...message, parts }) };
      setMessages((current) => {
        const nextMessages = current.map((item) => item.id === message.id ? updated : item);
        messagesRef.current = nextMessages;
        return nextMessages;
      });
      return updated;
    },
    [agent, gatewayClient, namespace, sessionId],
  );

  const startEditingMessage = useCallback((message: CopilotMessage) => {
    setEditingMessageId(message.id);
    setEditingMessageValue(editableMessageContent(message));
  }, []);

  const cancelEditingMessage = useCallback(() => {
    setEditingMessageId(null);
    setEditingMessageValue("");
  }, []);

  const saveEditingMessage = useCallback(async (message: CopilotMessage) => {
    const nextContent = editingMessageValue.trim();
    if (!nextContent) {
      return;
    }
    setError(null);
    const shouldPersistSessionEdit =
      enableDebugMessageEditing &&
      (message.role === "user" || message.role === "assistant") &&
      !isLocalMessageId(message.id);
    try {
      if (shouldPersistSessionEdit) {
        setReviewActionMessageId(message.id);
        await updateSessionMessage(message, replaceMessageTextPart(message, nextContent), { ...(message.labels ?? {}) });
        cancelEditingMessage();
        return;
      }
      const handled = await onMessageEdit?.({
        message,
        nextContent,
        namespace,
        agent,
        sessionId: currentSessionRef.current?.sessionId ?? sessionId ?? null,
      });
      if (handled === false) {
        return;
      }
      setMessages((prev) => {
        const nextMessages = prev.map((item) => item.id === message.id ? messageWithEditedContent(item, nextContent) : item);
        messagesRef.current = nextMessages;
        return nextMessages;
      });
      cancelEditingMessage();
    } catch (err) {
      setError(err instanceof Error ? err : new Error(String(err)));
    } finally {
      setReviewActionMessageId(null);
    }
  }, [agent, cancelEditingMessage, editingMessageValue, enableDebugMessageEditing, namespace, onMessageEdit, sessionId, updateSessionMessage]);

  const updateConnectorDeliveryStatus = useCallback(
    async (message: CopilotMessage, status: string) => {
      const labels = {
        ...(message.labels ?? {}),
        [LABEL_CONNECTOR_DELIVERY_STATUS]: status,
      };
      delete labels[LABEL_CONNECTOR_DELIVERY_ERROR];
      setError(null);
      setReviewActionMessageId(message.id);
      try {
        await updateSessionMessage(message, messagePartsForSessionUpdate(message), labels);
      } catch (err) {
        setError(err instanceof Error ? err : new Error(String(err)));
      } finally {
        setReviewActionMessageId(null);
      }
    },
    [updateSessionMessage],
  );

  const loadResource = useCallback(
    async (uri: string) => {
      resourceAbortRef.current?.abort();
      const controller = new AbortController();
      resourceAbortRef.current = controller;
      setResourceLoading(true);
      setResourceError(null);
      setResourceView(null);

      try {
        let view: ResourceViewModel;
        if (fetchResource) {
          view = await fetchResource(uri, controller.signal);
        } else {
          view = await fetchResourceFromGateway({
            uri,
            gatewayClient,
            agent,
            sessionId: currentSessionRef.current?.sessionId ?? sessionId ?? null,
            signal: controller.signal,
          });
        }
        if (controller.signal.aborted) return;
        setResourceView(view);
      } catch (err) {
        if (controller.signal.aborted) return;
        setResourceError(err instanceof Error ? err : new Error(String(err)));
      } finally {
        if (!controller.signal.aborted) {
          setResourceLoading(false);
        }
      }
    },
    [agent, fetchResource, gatewayClient, sessionId],
  );

  const clearResourcePaneState = useCallback(() => {
    resourceAbortRef.current?.abort();
    resourceAbortRef.current = null;
    setOpenResourceUri(null);
    setResourcePaneOpen(false);
    setResourceView(null);
    setResourceError(null);
    setResourceLoading(false);
  }, []);

  const closeResourcePane = useCallback(() => {
    // Animate closed; unmount after ResourcePane fires onExitComplete.
    resourceAbortRef.current?.abort();
    resourceAbortRef.current = null;
    setResourcePaneOpen(false);
  }, []);

  const handleResourcePaneExitComplete = useCallback(() => {
    setOpenResourceUri(null);
    setResourceView(null);
    setResourceError(null);
    setResourceLoading(false);
  }, []);

  const handleResourceClick = useCallback(
    (uri: string) => {
      if (onResourceClickProp) {
        onResourceClickProp(uri);
        return;
      }

      const parsed = parseResourceUri(uri);
      if (!parsed) return;

      if (openResourceUri === parsed.uri && resourcePaneOpen) {
        closeResourcePane();
        return;
      }

      const canFetchArtifact =
        Boolean(fetchResource) || Boolean(gatewayClient?.artifacts?.readArtifact);
      const canFetchFile = Boolean(fetchResource) || Boolean(gatewayClient?.files?.readFile);
      const canOpen =
        (parsed.kind === "artifact" && canFetchArtifact) ||
        (parsed.kind === "file" && canFetchFile);

      if (!canOpen) {
        if (!missingResourceClientWarnedRef.current && typeof console !== "undefined") {
          missingResourceClientWarnedRef.current = true;
          console.warn(
            "[talon-chat] Resource link clicked but no artifacts/files client (or fetchResource) is available.",
          );
        }
        return;
      }

      setOpenResourceUri(parsed.uri);
      setResourcePaneOpen(true);
      void loadResource(parsed.uri);
    },
    [
      closeResourcePane,
      fetchResource,
      gatewayClient,
      loadResource,
      onResourceClickProp,
      openResourceUri,
      resourcePaneOpen,
    ],
  );

  // Reset open resource pane when the session identity changes.
  useEffect(() => {
    clearResourcePaneState();
  }, [namespace, agent, sessionId]); // eslint-disable-line react-hooks/exhaustive-deps

  const copyMessageContent = useCallback(async (message: CopilotMessage) => {
    const nextContent = editableMessageContent(message);
    if (!nextContent.trim()) {
      return;
    }
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error("Clipboard API is unavailable.");
      }
      await navigator.clipboard.writeText(nextContent);
    } catch {
      const selection = window.getSelection();
      const textArea = document.createElement("textarea");
      textArea.value = nextContent;
      textArea.setAttribute("readonly", "");
      textArea.style.position = "fixed";
      textArea.style.left = "-9999px";
      document.body.appendChild(textArea);
      textArea.select();
      document.execCommand("copy");
      document.body.removeChild(textArea);
      selection?.removeAllRanges();
    }
  }, []);

  const renderedMessages = useMemo(() => {
    return messages.map((message, messageIndex) => {
      const content = getMessageContent(message);
      const images = messageImageParts(message, objectUrlForRef);
      const timeline = coalesceAssistantTimelineForDisplay(getMessageAssistantTimeline(message));
      const reasoningContent = getMessageReasoningContent(message);
      const usage = getMessageUsage(message);
      const usageSummary = formatUsageSummary(usage);
      const isUserMessage = message.role === "user";
      const isLatestMessage = messageIndex === messages.length - 1;
      const isLiveAssistantMessage = isSessionLive && isLatestMessage && message.role === "assistant";
      const isEditableMessage =
        (allowMessageEditing || enableDebugMessageEditing) &&
        (message.role === "user" || message.role === "assistant") &&
        !isLiveAssistantMessage;
      const isEditingMessage = editingMessageId === message.id;
      const messageActionTimestamp = isEditableMessage ? formatMessageActionTimestamp(message) : null;
      const finalizedTimeline = splitFinalAssistantTimeline(timeline);
      const visibleTimeline = finalizedTimeline.finalTimeline;
      const workTimeline = finalizedTimeline.workTimeline;
      const workHasReasoning = workTimeline.some((item) => item.type === "reasoning");
      const workHasUsage = workTimeline.some((item) => item.type === "usage");
      const hasExpandedWorkDetails =
        workTimeline.length > 0 ||
        (!workHasReasoning && Boolean(reasoningContent)) ||
        (!workHasUsage && Boolean(usageSummary));
      const hasWorkDetails = message.role === "assistant" && (hasExpandedWorkDetails || isLiveAssistantMessage);
      let previousUserMessage: CopilotMessage | undefined;
      if (message.role === "assistant") {
        for (let index = messageIndex - 1; index >= 0; index -= 1) {
          if (messages[index].role === "user") {
            previousUserMessage = messages[index];
            break;
          }
        }
      }
      const workLabel = isLiveAssistantMessage
        ? formatWorkingDuration(loadingStartedAt, loadingNow)
        : formatWorkDuration(previousUserMessage?.createdAt, message.createdAt);
      const isWorkExpanded = isLiveAssistantMessage || (expandedThinkingMessages[message.id] ?? false);
      const deliveryStatus = message.labels?.[LABEL_CONNECTOR_DELIVERY_STATUS];
      const isPendingConnectorDelivery =
        enableDebugMessageEditing && deliveryStatus === CONNECTOR_DELIVERY_PENDING_REVIEW;
      const isReviewActionPending = reviewActionMessageId === message.id;
      return (
        <React.Fragment key={message.id}>
          <div
          className="talon-session-message-row"
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
            }}
          >
            <div
              style={{
                overflow: "hidden",
                borderRadius: isUserMessage ? 18 : 0,
                background: isUserMessage
                  ? "var(--talon-chat-user-bubble-bg, rgba(24,24,27,0.07))"
                  : "transparent",
                color: isUserMessage ? "var(--talon-chat-user-bubble-fg, inherit)" : "inherit",
                padding: isUserMessage ? "0.75rem 1rem" : 0,
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
                  <div style={{ display: "flex", flexDirection: "column", gap: 8, paddingTop: 12 }}>
                    {workTimeline.map((item, index) => {
                      if (item.type === "compaction") {
                        return (
                          <ContextCompactionDivider
                            key={`${message.id}-work-${index}`}
                            compacting={isLiveAssistantMessage}
                          />
                        );
                      }

                      if (item.type === "text") {
                        return (
                          <div key={`${message.id}-work-${index}`} style={{ whiteSpace: "normal", overflowWrap: "break-word", fontSize: 13, lineHeight: 1.55, color: "var(--talon-chat-assistant-fg, inherit)" }}>
                            <MarkdownMessage onResourceClick={handleResourceClick}>{item.text}</MarkdownMessage>
                          </div>
                        );
                      }

                      if (item.type === "reasoning") {
                        return (
                          <div key={`${message.id}-work-${index}`} style={{ whiteSpace: "normal", overflowWrap: "break-word", fontSize: 13, lineHeight: 1.55, color: "var(--talon-chat-subtle-fg, rgba(82,82,91,0.96))" }}>
                            {item.text}
                          </div>
                        );
                      }

                      if (item.type === "usage") {
                        const itemUsageSummary = formatUsageSummary(item.usage);
                        return itemUsageSummary ? (
                          <div key={`${message.id}-work-${index}`} style={{ fontSize: 12, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>
                            {itemUsageSummary}
                          </div>
                        ) : null;
                      }

                      const toolKey = `${message.id}-work-tool-${item.toolCallId || index}`;
                      const isToolExpanded = expandedToolItems[toolKey] ?? false;
                      const isRunningTool = isLiveAssistantMessage && item.result === undefined;
                      const toolHydrationState = toolResultHydration[toolKey];
                      return (
                        <div key={toolKey}>
                          <button
                            className="talon-session-tool-row"
                            type="button"
                            onClick={() => {
                              toggleToolItem(toolKey);
                              if (!isToolExpanded) {
                                void hydrateToolResultForExpandedItem(message, item.toolCallId, toolKey, item.result);
                              }
                            }}
                            style={{
                              width: "auto",
                              maxWidth: "100%",
                              display: "flex",
                              alignItems: "center",
                              gap: 8,
                              border: "none",
                              background: "transparent",
                              padding: "0.25rem 0",
                              color: "var(--talon-chat-subtle-fg, rgba(82,82,91,0.96))",
                              cursor: "pointer",
                              textAlign: "left",
                            }}
                          >
                            <Wrench size="14" strokeWidth={1.9} style={{ flexShrink: 0, color: "var(--talon-chat-subtle-fg, rgba(113,113,122,0.9))" }} />
                            <span style={{ minWidth: 0, fontSize: 13, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                              Called <span style={{ fontFamily: "ui-monospace, SFMono-Regular, monospace" }}>{item.toolName}</span>
                            </span>
                            {isRunningTool ? (
                              <span style={{ flexShrink: 0, borderRadius: 999, background: "var(--talon-chat-tool-running-bg, rgba(14,165,233,0.12))", color: "var(--talon-chat-tool-running-fg, #0369a1)", padding: "0.1rem 0.45rem", fontSize: 11, fontWeight: 700 }}>
                                Running
                              </span>
                            ) : null}
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
                              {toolHydrationState ? (
                                <div style={{ fontSize: 12, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>
                                  {toolHydrationState === "loading" ? "Loading output..." : "Could not load output."}
                                </div>
                              ) : item.result !== undefined ? (
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

                    {!workHasReasoning && reasoningContent ? (
                      <div style={{ whiteSpace: "normal", overflowWrap: "break-word", fontSize: 13, lineHeight: 1.55, color: "var(--talon-chat-subtle-fg, rgba(82,82,91,0.96))" }}>
                        {reasoningContent}
                      </div>
                    ) : null}

                    {!workHasUsage && usageSummary ? (
                      <div style={{ fontSize: 12, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>
                        {usageSummary}
                      </div>
                    ) : null}
                  </div>
                ) : null}
              </div>
            ) : null}

            {isPendingConnectorDelivery ? (
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: 8,
                  marginBottom: 8,
                  color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))",
                  fontSize: 12,
                }}
              >
                <span>Pending send</span>
                <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <button
                    type="button"
                    disabled={isReviewActionPending || isEditingMessage}
                    onClick={() => void updateConnectorDeliveryStatus(message, CONNECTOR_DELIVERY_REQUESTED)}
                    style={{ border: "none", background: "transparent", color: "var(--talon-chat-accent-fg, #047857)", cursor: isReviewActionPending || isEditingMessage ? "not-allowed" : "pointer", fontWeight: 700, padding: "2px 4px", opacity: isReviewActionPending || isEditingMessage ? 0.55 : 1 }}
                  >
                    Send
                  </button>
                  <button
                    type="button"
                    disabled={isReviewActionPending || isEditingMessage}
                    onClick={() => void updateConnectorDeliveryStatus(message, CONNECTOR_DELIVERY_SKIPPED)}
                    style={{ border: "none", background: "transparent", color: "inherit", cursor: isReviewActionPending || isEditingMessage ? "not-allowed" : "pointer", padding: "2px 4px", opacity: isReviewActionPending || isEditingMessage ? 0.55 : 1 }}
                  >
                    Skip
                  </button>
                </div>
              </div>
            ) : null}

            {isEditingMessage ? (
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                <textarea
                  className="talon-session-edit-textarea"
                  aria-label="Edit message"
                  value={editingMessageValue}
                  onChange={(event) => setEditingMessageValue(event.currentTarget.value)}
                  rows={Math.min(8, Math.max(2, editingMessageValue.split("\n").length))}
                  style={{
                    width: "100%",
                    resize: "vertical",
                    border: border("var(--talon-chat-edit-border, rgba(82,82,91,0.86))"),
                    borderRadius: 8,
                    background: "var(--talon-chat-edit-bg, rgba(24,24,27,0.92))",
                    color: "var(--talon-chat-edit-fg, inherit)",
                    padding: "0.65rem 0.8rem",
                    font: "inherit",
                    fontSize: talonChatMessageFontSize,
                    lineHeight: 1.55,
                    outline: "none",
                    boxShadow: "var(--talon-chat-edit-shadow, inset 0 0 0 1px rgba(255,255,255,0.02))",
                  }}
                />
                <div style={{ display: "flex", justifyContent: isUserMessage ? "flex-end" : "flex-start", gap: 6 }}>
                  <button
                    className="talon-session-edit-action"
                    type="button"
                    aria-label="Save message edit"
                    title="Save"
                    onClick={() => void saveEditingMessage(message)}
                    disabled={!editingMessageValue.trim()}
                    style={{
                      width: 28,
                      height: 28,
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      borderRadius: 8,
                      border: border("var(--talon-chat-edit-action-border, rgba(82,82,91,0.82))"),
                      background: "var(--talon-chat-edit-action-bg, rgba(39,39,42,0.92))",
                      color: "var(--talon-chat-edit-action-fg, inherit)",
                      cursor: editingMessageValue.trim() ? "pointer" : "not-allowed",
                      opacity: editingMessageValue.trim() ? 1 : 0.45,
                    }}
                  >
                    <Check size="14" strokeWidth={2} />
                  </button>
                  <button
                    className="talon-session-edit-action"
                    type="button"
                    aria-label="Cancel message edit"
                    title="Cancel"
                    onClick={cancelEditingMessage}
                    style={{
                      width: 28,
                      height: 28,
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      borderRadius: 8,
                      border: border("var(--talon-chat-edit-action-border, rgba(82,82,91,0.82))"),
                      background: "var(--talon-chat-edit-action-bg, rgba(39,39,42,0.92))",
                      color: "var(--talon-chat-edit-action-fg, inherit)",
                      cursor: "pointer",
                    }}
                  >
                    <X size="14" strokeWidth={2} />
                  </button>
                </div>
              </div>
            ) : (
              <div
                className={cn(message.role === "system" && "copilot-system-message")}
                style={{
                  minWidth: 0,
                  overflow: "hidden",
                  overflowWrap: "anywhere",
                  whiteSpace: message.role === "assistant" ? "normal" : "pre-wrap",
                  fontSize: message.role === "system" ? 12 : talonChatMessageFontSize,
                  lineHeight: 1.65,
                  opacity: message.role === "system" ? 0.72 : 0.94,
                  fontFamily: message.role === "system" ? "ui-monospace, SFMono-Regular, monospace" : undefined,
                }}
              >
                {message.role === "assistant" && visibleTimeline.length > 0 ? (
                  <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                    {visibleTimeline.map((item, index) => {
                      if (item.type === "compaction") {
                        return (
                          <ContextCompactionDivider
                            key={`${message.id}-timeline-${index}`}
                            compacting={isLiveAssistantMessage}
                          />
                        );
                      }

                      if (item.type === "text") {
                        return (
                          <div key={`${message.id}-timeline-${index}`} style={{ whiteSpace: "normal", overflowWrap: "anywhere" }}>
                            <MarkdownMessage onResourceClick={handleResourceClick}>{item.text}</MarkdownMessage>
                          </div>
                        );
                      }

                      if (item.type === "reasoning") {
                        return (
                          <div key={`${message.id}-timeline-${index}`} style={{ whiteSpace: "normal", overflowWrap: "break-word", color: "var(--talon-chat-subtle-fg, rgba(82,82,91,0.96))" }}>
                            {item.text}
                          </div>
                        );
                      }

                      if (item.type === "usage") {
                        const itemUsageSummary = formatUsageSummary(item.usage);
                        return itemUsageSummary ? (
                          <div key={`${message.id}-timeline-${index}`} style={{ fontSize: 12, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>
                            {itemUsageSummary}
                          </div>
                        ) : null;
                      }

                      const toolKey = `${message.id}-timeline-tool-${item.toolCallId || index}`;
                      const isToolExpanded = expandedToolItems[toolKey] ?? false;
                      const isRunningTool = isLiveAssistantMessage && item.result === undefined;
                      const toolHydrationState = toolResultHydration[toolKey];
                      return (
                        <div key={toolKey}>
                          <button
                            className="talon-session-tool-row"
                            type="button"
                            onClick={() => {
                              toggleToolItem(toolKey);
                              if (!isToolExpanded) {
                                void hydrateToolResultForExpandedItem(message, item.toolCallId, toolKey, item.result);
                              }
                            }}
                            style={{
                              width: "auto",
                              maxWidth: "100%",
                              display: "flex",
                              alignItems: "center",
                              gap: 8,
                              border: "none",
                              background: "transparent",
                              padding: "0.25rem 0",
                              color: "var(--talon-chat-subtle-fg, rgba(82,82,91,0.96))",
                              cursor: "pointer",
                              textAlign: "left",
                            }}
                          >
                            <Wrench size="14" strokeWidth={1.9} style={{ flexShrink: 0, color: "var(--talon-chat-subtle-fg, rgba(113,113,122,0.9))" }} />
                            <span style={{ minWidth: 0, fontSize: 13, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                              Called <span style={{ fontFamily: "ui-monospace, SFMono-Regular, monospace" }}>{item.toolName}</span>
                            </span>
                            {isRunningTool ? (
                              <span style={{ flexShrink: 0, borderRadius: 999, background: "var(--talon-chat-tool-running-bg, rgba(14,165,233,0.12))", color: "var(--talon-chat-tool-running-fg, #0369a1)", padding: "0.1rem 0.45rem", fontSize: 11, fontWeight: 700 }}>
                                Running
                              </span>
                            ) : null}
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
                              {toolHydrationState ? (
                                <div style={{ fontSize: 12, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>
                                  {toolHydrationState === "loading" ? "Loading output..." : "Could not load output."}
                                </div>
                              ) : item.result !== undefined ? (
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
                  </div>
                ) : (
                  message.role === "assistant" ? (
                    <MarkdownMessage onResourceClick={handleResourceClick}>{content}</MarkdownMessage>
                  ) : content
                )}
              </div>
            )}
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
            {isEditableMessage && !isEditingMessage ? (
              <div
                className={cn("talon-session-message-actions", isUserMessage && "talon-session-message-actions-user")}
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: isUserMessage ? "flex-end" : "flex-start",
                  gap: 10,
                  marginTop: 6,
                  minHeight: 22,
                }}
              >
                {messageActionTimestamp ? (
                  <span
                    className="talon-session-message-action-time"
                    title={new Date(historyMessageTimestamp(message) ?? 0).toLocaleString()}
                    style={{
                      color: "var(--talon-chat-message-action-fg, var(--talon-chat-muted-fg, rgba(113,113,122,0.9)))",
                      fontSize: 12,
                      lineHeight: 1,
                      whiteSpace: "nowrap",
                    }}
                  >
                    {messageActionTimestamp}
                  </span>
                ) : null}
                <button
                  className="talon-session-message-action-button"
                  type="button"
                  aria-label={`Copy ${message.role} message`}
                  title="Copy"
                  onClick={() => void copyMessageContent(message)}
                  style={{
                    width: 22,
                    height: 22,
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    borderRadius: 6,
                    border: "none",
                    background: "transparent",
                    color: "var(--talon-chat-message-action-fg, var(--talon-chat-muted-fg, rgba(113,113,122,0.9)))",
                    cursor: "pointer",
                  }}
                >
                  <Copy size="14" strokeWidth={1.9} />
                </button>
                <button
                  className="talon-session-edit-trigger talon-session-message-action-button"
                  type="button"
                  aria-label={`Edit ${message.role} message`}
                  title="Edit"
                  onClick={() => startEditingMessage(message)}
                  style={{
                    width: 22,
                    height: 22,
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    borderRadius: 6,
                    border: "none",
                    background: "transparent",
                    color: "var(--talon-chat-message-action-fg, var(--talon-chat-muted-fg, rgba(113,113,122,0.9)))",
                    cursor: "pointer",
                  }}
                >
                  <Pencil size="14" strokeWidth={1.9} />
                </button>
              </div>
            ) : null}
          </div>
          </div>
        </React.Fragment>
      );
    });
  }, [allowMessageEditing, cancelEditingMessage, copyMessageContent, editingMessageId, editingMessageValue, enableDebugMessageEditing, expandedThinkingMessages, expandedToolItems, handleResourceClick, hydrateToolResultForExpandedItem, isLoading, isResuming, isSessionLive, isStopping, loadingNow, loadingStartedAt, messages, objectUrlForRef, reviewActionMessageId, saveEditingMessage, startEditingMessage, toggleThinkingMessage, toggleToolItem, toolResultHydration, updateConnectorDeliveryStatus]);

  const resolvedHistoryPageSize = Math.max(
    1,
    Math.trunc(historyPageSize || historyMessageLimit || DEFAULT_HISTORY_PAGE_SIZE),
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
        const res = await loadOlderRuntime(target);
        if (!res) {
          prependScrollRestoreRef.current = null;
          skipNextAutoScrollRef.current = false;
        }
      } catch (err) {
        prependScrollRestoreRef.current = null;
        skipNextAutoScrollRef.current = false;
        console.warn("Could not load older session history", err);
      } finally {
        isLoadingOlderHistoryRef.current = false;
        setIsLoadingOlderHistory(false);
      }
    },
    [loadOlderRuntime, nextBeforeMessageId],
  );

  const refreshNewestSessionPage = useCallback(
    async (target: { ns: string; agent: string; sessionId: string }, signal?: AbortSignal) => {
      setStreamEvents([]);
      return refreshRuntime(target, signal);
    },
    [refreshRuntime],
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
          signal,
        });
      } catch (err) {
        if (!signal?.aborted) {
          setError(err instanceof Error ? err : new Error(String(err)));
        }
      } finally {
        if (!signal?.aborted && isSameSession(currentSessionRef.current, target)) {
          const refreshed = await refreshNewestSessionPage(target).catch(() => null);
          if (refreshed?.state === "PROCESSING" && !isStoppingRef.current) {
            const controller = new AbortController();
            resumeAbortControllerRef.current?.abort();
            resumeAbortControllerRef.current = controller;
            setIsResuming(true);
            setLoadingStartedAt(sessionProcessingStartTime(refreshed.messages) ?? Date.now());
            setLoadingNow(Date.now());
            window.setTimeout(() => {
              if (!controller.signal.aborted && isSameSession(currentSessionRef.current, target) && !isStoppingRef.current) {
                void resumeStream(target, controller.signal);
              }
            }, 250);
          } else {
            setIsResuming(false);
            setLoadingStartedAt(null);
          }
        }
      }
    },
    [gatewayClient, refreshNewestSessionPage],
  );

  const waitForSessionToStop = useCallback(
    async (target: { ns: string; agent: string; sessionId: string }, signal?: AbortSignal) => {
      for (let attempt = 0; attempt < 40; attempt += 1) {
        if (signal?.aborted || !isSameSession(currentSessionRef.current, target)) {
          return null;
        }
        const res = await refreshRuntime(target, signal);
        if (signal?.aborted || !isSameSession(currentSessionRef.current, target)) {
          return null;
        }
        if (res?.state !== "PROCESSING") {
          return res;
        }
        await new Promise((resolve) => setTimeout(resolve, 250));
      }
      return null;
    },
    [refreshRuntime],
  );

  useEffect(() => {
    if (!currentSession || sessionRuntimeState.serverState !== "PROCESSING" || isStoppingRef.current) {
      return;
    }
    if (resumeAbortControllerRef.current && !resumeAbortControllerRef.current.signal.aborted) return;
    const controller = new AbortController();
    resumeAbortControllerRef.current = controller;
    setIsResuming(true);
    setLoadingStartedAt(sessionProcessingStartTime(messagesRef.current) ?? Date.now());
    setLoadingNow(Date.now());
    void resumeStream(currentSession, controller.signal);
    return () => controller.abort();
  }, [currentSession, messagesRef, resumeStream, sessionRuntimeState.serverState, setIsResuming]);

  const waitForCanonicalAssistantUpdate = useCallback(
    async (session: { ns: string; agent: string; sessionId: string }, baselineSignature: string, signal?: AbortSignal) => {
      for (let attempt = 0; attempt < 40; attempt += 1) {
        if (signal?.aborted || !isSameSession(currentSessionRef.current, session)) {
          return false;
        }
        const sessionState = await refreshRuntime(session, signal);
        if (signal?.aborted || !isSameSession(currentSessionRef.current, session)) {
          return false;
        }
        if (!sessionState) return false;
        const nextSignature = getAssistantSignature(sessionState.messages);
        if (nextSignature && nextSignature !== baselineSignature) {
          await refreshNewestSessionPage(session, signal);
          return true;
        }
        await new Promise((resolve) => setTimeout(resolve, 250));
      }
      return false;
    },
    [refreshNewestSessionPage, refreshRuntime],
  );

  const clearLocalSession = useCallback(() => {
    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    resumeAbortControllerRef.current?.abort();
    resumeAbortControllerRef.current = null;
    stopAbortControllerRef.current?.abort();
    stopAbortControllerRef.current = null;
    resourceAbortRef.current?.abort();
    resourceAbortRef.current = null;
    clearRuntime();
    messagesRef.current = emptyMessages;
    setStreamEvents([]);
    setError(null);
    setIsLoading(false);
    setIsResuming(false);
    setSessionState(null);
    isStoppingRef.current = false;
    setIsStopping(false);
    setLoadingStartedAt(null);
    setExpandedThinkingMessages({});
    setExpandedToolItems({});
    invalidateToolResultHydration();
    setOpenResourceUri(null);
    setResourceView(null);
    setResourceError(null);
    setResourceLoading(false);
    autoScrollPinnedRef.current = true;
  }, [clearRuntime, invalidateToolResultHydration]);

  const clearSession = useCallback(async () => {
    const session = currentSessionRef.current;
    if (session) {
      try {
        const sessions = gatewayClient?.sessions;
        if (!sessions?.clear) {
          throw new Error("TalonSession requires a Talon clientset with sessions.clear().");
        }
        await sessions.clear(session);
      } catch (err) {
        setError(err instanceof Error ? err : new Error(String(err)));
      }
    }
    clearLocalSession();
  }, [clearLocalSession, gatewayClient, setError]);

  const resolvedCommands = useMemo<Array<TalonSessionCommand>>(() => {
    const builtInCommands: TalonSessionCommand[] = [];
    if (enabledBuiltInCommands?.includes("clear")) {
      builtInCommands.push({
        name: "clear",
        description: "Clear the current session history.",
        run: ({ clear }) => clear?.(),
      });
    }
    if (enabledBuiltInCommands?.includes("goal")) {
      builtInCommands.push({
        name: "goal",
        description: "Create or update a session Goal.",
        run: () => undefined,
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
    let text = submittedText.trim();
    const pendingAttachments = imageAttachmentsRef.current;
    const hasImages = pendingAttachments.length > 0;
    if ((!text && !hasImages) || isSessionLive || disabled) return;
    let submitTurnStarted = false;
    let resumedAfterBusyFailure = false;
    let submittedUserMessageId: string | null = null;
    let submittedSession: TalonSessionHandle | null = null;
    let submitController: AbortController | null = null;

    const ensureSession = async (): Promise<TalonSessionHandle> => {
      let session = currentSessionRef.current;
      if (!session) {
        if (sessionId) {
          session = { ns: namespace, agent, sessionId };
          currentSessionRef.current = session;
          activateTarget(session, { hydrate: false });
        } else {
          const sessionRes = await createSession({ ns: namespace, agent });
          session = { ns: namespace, agent, sessionId: sessionRes.sessionId };
          currentSessionRef.current = session;
          activateTarget(session, { hydrate: false });
          onSessionChange?.(session.sessionId);
        }
      }
      return session;
    };

    if (onSubmitMessage) {
      setError(null);
      try {
        const handled = await onSubmitMessage({
          text,
          namespace,
          agent,
          sessionId: currentSessionRef.current?.sessionId ?? sessionId ?? null,
          imageAttachments: pendingAttachments,
          ensureSession,
          clearInput: () => setInput(""),
          refreshSession: async () => {
            const session = await ensureSession();
            await refreshNewestSessionPage(session);
          },
        });
        if (handled) {
          return;
        }
      } catch (err) {
        setError(err instanceof Error ? err : new Error(String(err)));
        return;
      }
    }

    let parsedCommand = parseTalonChatCommandInput(text);
    const isGoalCommand =
      parsedCommand?.name === "goal" && enabledBuiltInCommands?.includes("goal");
    if (isGoalCommand) {
      const goalText = parsedCommand.args?.trim() ?? "";
      if (!goalText) {
        setError(new Error("Usage: /goal <objective and success criteria>"));
        return;
      }
      text = [
        "Create or update a Talon Goal for this session.",
        "",
        "Use the goal tools directly. Track this objective until completion:",
        goalText,
      ].join("\n");
      parsedCommand = null;
    }

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
    setIsResuming(false);

    try {
      let session = currentSessionRef.current;
      const baselineAssistantSignature = getAssistantSignature(
        messagesRef.current.slice(-resolvedHistoryPageSize),
      );

      session = await ensureSession();
      submittedSession = session;

      const controller = new AbortController();
      submitController = controller;
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
      submittedUserMessageId = userMessage.id;

      setInput("");
      submittedPreviewUrlsRef.current.push(...uploadedImages.map((attachment) => attachment.previewUrl));
      setImageAttachments([]);
      setMessages((prev) => [...prev, userMessage]);
      setLoadingStartedAt(normalizeEpochToMilliseconds(userMessage.createdAt) ?? Date.now());
      setLoadingNow(Date.now());
      autoScrollPinnedRef.current = true;
      setIsLoading(true);

      const sessions = gatewayClient?.sessions;
      if (!sessions?.submitTurn) {
        throw new Error("TalonSession requires a Talon clientset with sessions.submitTurn().");
      }

      const turnStream = sessions.submitTurn({
        ns: session.ns,
        agent: session.agent,
        sessionId: session.sessionId,
        message: {
          role: data.MessageRole.ROLE_USER,
          parts: protoSessionPartsFromChatParts(userMessage.parts),
        },
        labels: {},
      }, { signal: controller.signal });

      submitTurnStarted = true;
      const { hasAssistantEvent } = await streamSessionPartEvents({
        events: turnStream,
        setMessages,
        setStreamEvents,
        signal: controller.signal,
      });

      if (!hasAssistantEvent) {
        await waitForCanonicalAssistantUpdate(session, baselineAssistantSignature, submitController?.signal);
      } else {
        await refreshNewestSessionPage(session, submitController?.signal);
      }
    } catch (err: any) {
      const nextError = err instanceof Error ? err : new Error(String(err));
      const session = submittedSession && isSameSession(currentSessionRef.current, submittedSession)
        ? submittedSession
        : null;
      if (submitController?.signal.aborted || (submittedSession && !session)) {
        return;
      }
      if (session && isSessionBusyError(nextError)) {
        if (submittedUserMessageId) {
          const optimisticMessageId = submittedUserMessageId;
          messagesRef.current = messagesRef.current.filter((message) => message.id !== optimisticMessageId);
          setMessages((prev) => prev.filter((message) => message.id !== optimisticMessageId));
          setInput((current) => current || submittedText.trim());
          if (imageAttachmentsRef.current.length === 0 && pendingAttachments.length > 0) {
            imageAttachmentsRef.current = pendingAttachments;
            setImageAttachments(pendingAttachments);
          }
        }
        const refreshed = await refreshNewestSessionPage(session, submitController?.signal).catch(() => null);
        if (submitController?.signal.aborted || !isSameSession(currentSessionRef.current, session)) {
          return;
        }
        if (isStoppingRef.current) {
          return;
        }
        if (refreshed?.state === "PROCESSING") {
          const controller = new AbortController();
          resumeAbortControllerRef.current?.abort();
          resumeAbortControllerRef.current = controller;
          resumedAfterBusyFailure = true;
          setIsResuming(true);
          setLoadingStartedAt(sessionProcessingStartTime(refreshed.messages) ?? Date.now());
          setLoadingNow(Date.now());
          setError(null);
          void resumeStream(session, controller.signal);
          return;
        }
      }
      if (session && submitTurnStarted && !isSessionBusyError(nextError)) {
        const baselineAssistantSignature = getAssistantSignature(
          messagesRef.current.slice(-resolvedHistoryPageSize),
        );
        const recovered = await waitForCanonicalAssistantUpdate(session, baselineAssistantSignature, submitController?.signal).catch(() => false);
        if (recovered) {
          setError(null);
          return;
        }
      }
      setError(nextError);
    } finally {
      const staleSession = submitController?.signal.aborted
        || (submittedSession && !isSameSession(currentSessionRef.current, submittedSession));
      if (!staleSession && (!submitController || abortControllerRef.current === submitController)) {
        abortControllerRef.current = null;
        setIsLoading(false);
        if (!resumedAfterBusyFailure) {
          setLoadingStartedAt(null);
        }
      }
    }
  }, [agent, clearSession, createSession, disabled, gatewayClient, isLoading, isSessionLive, namespace, onSessionChange, refreshNewestSessionPage, resolvedCommands, resolvedHistoryPageSize, resumeStream, sessionId, uploadQueuedImages, waitForCanonicalAssistantUpdate]);

  const stopGeneration = useCallback(async () => {
    if (!currentSessionRef.current || !isSessionLive || isStopping) return;

    const session = currentSessionRef.current;
    const stopController = new AbortController();
    stopAbortControllerRef.current?.abort();
    stopAbortControllerRef.current = stopController;
    isStoppingRef.current = true;
    setIsStopping(true);
    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    resumeAbortControllerRef.current?.abort();
    resumeAbortControllerRef.current = null;
    setIsLoading(false);
    setIsResuming(false);
    setLoadingStartedAt(null);

    const resumeIfStillProcessing = async () => {
      const refreshed = await refreshNewestSessionPage(session, stopController.signal).catch(() => null);
      if (refreshed?.state !== "PROCESSING") {
        return;
      }
      const controller = new AbortController();
      resumeAbortControllerRef.current?.abort();
      resumeAbortControllerRef.current = controller;
      setIsResuming(true);
      setLoadingStartedAt(sessionProcessingStartTime(refreshed.messages) ?? Date.now());
      setLoadingNow(Date.now());
      void resumeStream(session, controller.signal);
    };

    try {
      const sessions = gatewayClient?.sessions;
      if (!sessions?.stopGeneration) {
        throw new Error("TalonSession requires a Talon clientset with sessions.stopGeneration().");
      }
      await sessions.stopGeneration(session, { signal: stopController.signal });
      const stopped = await waitForSessionToStop(session, stopController.signal);
      if (stopController.signal.aborted || !isSameSession(currentSessionRef.current, session)) {
        return;
      }
      if (!stopped) {
        setError(new Error("Stop was requested, but the session is still generating."));
        await resumeIfStillProcessing();
        return;
      }
      await refreshNewestSessionPage(session, stopController.signal);
      setIsResuming(false);
      setLoadingStartedAt(null);
      setError(null);
    } catch (err) {
      if (stopController.signal.aborted || !isSameSession(currentSessionRef.current, session)) {
        return;
      }
      const stopError = err instanceof Error ? err : new Error(String(err));
      setError(stopError);
      await resumeIfStillProcessing();
    } finally {
      if (stopAbortControllerRef.current === stopController) {
        stopAbortControllerRef.current = null;
      }
      if (!stopController.signal.aborted && isSameSession(currentSessionRef.current, session)) {
        isStoppingRef.current = false;
        setIsStopping(false);
      }
    }
  }, [gatewayClient, isSessionLive, isStopping, refreshNewestSessionPage, resumeStream, setError, waitForSessionToStop]);

  const handleTranscriptScroll = useCallback(() => {
    updateTranscriptScrollThumb();
    const container = scrollContainerRef.current;
    const session = currentSessionRef.current;
    if (container && !prependScrollRestoreRef.current) {
      autoScrollPinnedRef.current = isNearScrollBottom(container);
    }
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

          .talon-session-message-actions {
            opacity: 0;
            pointer-events: none;
            transition: opacity 120ms ease;
          }

          .talon-session-message-row:hover .talon-session-message-actions {
            opacity: 1;
            pointer-events: auto;
          }

          .talon-session-message-action-button {
            transition: color 120ms ease, background 120ms ease;
          }

          .talon-session-message-action-button:hover,
          .talon-session-message-action-button:focus-visible {
            background: var(--talon-chat-edit-trigger-hover-bg, rgba(113,113,122,0.14)) !important;
            color: var(--talon-chat-edit-trigger-hover-fg, var(--talon-chat-edit-trigger-fg, inherit)) !important;
          }

          .talon-session-edit-textarea::placeholder {
            color: var(--talon-chat-edit-placeholder-fg, rgba(161,161,170,0.72));
          }

          .talon-session-edit-textarea:focus {
            border-color: var(--talon-chat-edit-focus-border, rgba(161,161,170,0.95)) !important;
            box-shadow: var(--talon-chat-edit-focus-shadow, 0 0 0 2px rgba(161,161,170,0.2)) !important;
          }

          .talon-session-edit-action:hover:not(:disabled),
          .talon-session-edit-action:focus-visible:not(:disabled) {
            background: var(--talon-chat-edit-action-hover-bg, rgba(63,63,70,0.96)) !important;
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
      <div
        style={{
          display: "flex",
          flexDirection: "row",
          flex: 1,
          minHeight: 0,
          minWidth: 0,
          position: "relative",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            // Fills remaining space so chat + pane land at an even 50/50 when open.
            flex: "1 1 auto",
            minWidth: 0,
            minHeight: 0,
            transition: "flex 280ms cubic-bezier(0.22, 1, 0.36, 1)",
          }}
        >
          <SessionTranscript
            isLive={isSessionLive}
            hasTrailingUserMessage={messages[messages.length - 1]?.role === "user"}
            workingLabel={formatWorkingDuration(loadingStartedAt, loadingNow)}
            error={error}
            scrollThumb={scrollThumb}
            transcriptRef={scrollContainerRef}
            bottomRef={bottomRef}
            onScroll={handleTranscriptScroll}
          >
            {renderedMessages}
          </SessionTranscript>

          <SessionComposerDock
            disabled={disabled}
            value={input}
            onValueChange={setInput}
            onSubmit={(nextInput) => void submitMessage(nextInput)}
            placeholder={placeholder}
            variant={composerVariant}
            autoFocus={autoFocus}
            rows={inputRows}
            canSubmit={Boolean((input || "").trim() || imageAttachments.length > 0) && !isSessionLive}
            isGenerating={isSessionLive}
            canStop={Boolean(currentSession) && !isStopping}
            commandMenuItems={commandMenuItems}
            startAdornment={composerStartAdornment}
            endAdornment={composerEndAdornment}
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

        {openResourceUri ? (
          <ResourcePane
            uri={openResourceUri}
            resource={resourceView}
            isLoading={resourceLoading}
            error={resourceError}
            open={resourcePaneOpen}
            onClose={closeResourcePane}
            onExitComplete={handleResourcePaneExitComplete}
            onResourceClick={handleResourceClick}
          />
        ) : null}
      </div>
    </div>
  );
}

export const TalonCopilot = TalonSession;
