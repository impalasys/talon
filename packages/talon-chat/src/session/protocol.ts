import { data } from "@impalasys/talon-client";
import type { CopilotMessage } from "../lib/chatTimeline";
import type { TalonChatObjectRef, WireMessagePart } from "./types";

export const SESSION_MESSAGE_PART_TYPE = {
  TEXT: (data.SessionMessagePartType?.TEXT ?? "SESSION_MESSAGE_PART_TYPE_TEXT") as data.SessionMessagePartType,
  TOOL_RESULT: (data.SessionMessagePartType?.TOOL_RESULT ?? "SESSION_MESSAGE_PART_TYPE_TOOL_RESULT") as data.SessionMessagePartType,
  IMAGE: (data.SessionMessagePartType?.IMAGE ?? "SESSION_MESSAGE_PART_TYPE_IMAGE") as data.SessionMessagePartType,
};

export function parseToolResultPayload(payloadJson: unknown): Record<string, unknown> {
  if (typeof payloadJson !== "string" || payloadJson.length === 0) return {};
  try {
    const value = JSON.parse(payloadJson);
    return value && typeof value === "object" && !Array.isArray(value)
      ? value as Record<string, unknown>
      : {};
  } catch {
    return {};
  }
}

export const parsePayloadJson = parseToolResultPayload;

function getMessageContent(message: CopilotMessage): string {
  return typeof message.content === "string" ? message.content : "";
}

export function serializableMessageParts(parts: unknown): unknown[] {
  if (!Array.isArray(parts)) return [];
  return parts.map((part: unknown) => {
    if (!part || typeof part !== "object") return part;
    const { previewUrl: _previewUrl, ...serializablePart } = part as Record<string, unknown>;
    return serializablePart;
  });
}

export function wirePartToChatPart(part: unknown): Record<string, unknown> {
  return part && typeof part === "object" ? { ...(part as Record<string, unknown>) } : { content: String(part ?? "") };
}

export function chatPartToWirePart(part: unknown): WireMessagePart {
  if (part && typeof part === "object") {
    const { previewUrl: _previewUrl, ...wirePart } = part as Record<string, unknown>;
    return wirePart as WireMessagePart;
  }
  return {
    partType: SESSION_MESSAGE_PART_TYPE.TEXT,
    content: String(part ?? ""),
  };
}

export function protoSessionPartsFromChatParts(parts: unknown): any[] {
  return serializableMessageParts(parts).map((part: any) => {
    if (part?.type === "image") {
      return {
        partType: SESSION_MESSAGE_PART_TYPE.IMAGE,
        payloadJson: part.payloadJson ?? part.payload_json ?? "",
        object: part.object,
      };
    }
    return {
      partType: SESSION_MESSAGE_PART_TYPE.TEXT,
      content: String(part?.text ?? part?.content ?? ""),
    };
  });
}

export function chatMessageToWireMessage(message: CopilotMessage): Record<string, unknown> {
  return {
    id: message.id,
    role: message.role,
    parts: Array.isArray(message.parts)
      ? message.parts.map(chatPartToWirePart)
      : [{ partType: SESSION_MESSAGE_PART_TYPE.TEXT, content: getMessageContent(message) }],
  };
}

export function wireMessageToChatMessage(message: any): CopilotMessage {
  return {
    id: String(message?.id ?? ""),
    role: message?.role === "ROLE_USER" || message?.role === 1 || message?.role === "user" ? "user" : "assistant",
    content: typeof message?.content === "string" ? message.content : "",
    createdAt: message?.createdAt ?? message?.created_at,
    parts: Array.isArray(message?.parts) ? message.parts.map(wirePartToChatPart) : undefined,
  };
}

export function isSessionTextPart(part: unknown): boolean {
  const value = part as Record<string, unknown> | null;
  const type = value?.type ?? value?.partType ?? value?.part_type;
  return type === "text" || type === SESSION_MESSAGE_PART_TYPE.TEXT || type === "SESSION_MESSAGE_PART_TYPE_TEXT";
}

export function messagePartsForSessionUpdate(message: CopilotMessage): any[] {
  return Array.isArray(message.parts) && message.parts.length > 0
    ? message.parts.map(chatPartToWirePart)
    : [{ partType: SESSION_MESSAGE_PART_TYPE.TEXT, content: getMessageContent(message) }];
}

export function toolResultObjectRef(value: unknown, parsePayload = parseToolResultPayload): TalonChatObjectRef | undefined {
  if (!value || typeof value !== "object") return undefined;
  const candidate = value as Record<string, unknown>;
  const direct = candidate.object ?? candidate.objectRef ?? candidate.object_ref;
  if (direct && typeof direct === "object" && typeof (direct as TalonChatObjectRef).key === "string") {
    return direct as TalonChatObjectRef;
  }
  const payloadJson = candidate.payloadJson ?? candidate.payload_json;
  if (typeof payloadJson !== "string" || payloadJson.length === 0) return undefined;
  const payload = parsePayload(payloadJson);
  if (Object.keys(payload).length === 0) return undefined;
  return toolResultObjectRef(payload, parsePayload);
}
