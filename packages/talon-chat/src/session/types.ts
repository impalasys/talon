import type { CopilotMessage } from "../lib/chatTimeline";
import type { data } from "@impalasys/talon-client";

export type SessionTarget = {
  ns: string;
  agent: string;
  sessionId: string;
};

export type TalonChatObjectRef = {
  key: string;
  mediaType?: string;
  media_type?: string;
  sizeBytes?: number | bigint | string;
  size_bytes?: number | bigint | string;
  sha256?: string;
  filename?: string;
  metadata?: Record<string, string>;
  contentEncoding?: string;
  content_encoding?: string;
};

export type TalonSessionHandle = SessionTarget;

export type ChatMessagePart = Record<string, unknown> & {
  type?: string;
  partType?: unknown;
  part_type?: unknown;
  text?: string;
  content?: string;
  previewUrl?: string;
};

export type WireMessagePart = Record<string, unknown> & {
  partType?: data.SessionMessagePartType;
  part_type?: unknown;
  content?: string;
  payloadJson?: string;
  payload_json?: string;
  object?: TalonChatObjectRef;
};

export type WireSessionMessage = Record<string, unknown> & {
  id?: string;
  role?: unknown;
  parts?: unknown;
};

export type SessionMessageUpdate = {
  message: CopilotMessage;
  parts: WireMessagePart[];
};
