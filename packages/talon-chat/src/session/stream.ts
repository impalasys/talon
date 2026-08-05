import { data } from "@impalasys/talon-client";
import type { UsageSummary } from "../lib/chatTimeline";

export type ChatPart = Record<string, unknown> & {
  partType?: data.SessionMessagePartType | string;
  part_type?: data.SessionMessagePartType | string;
  id?: string;
  name?: string;
  toolCallId?: string;
  tool_call_id?: string;
  content?: string;
  payloadJson?: string;
  payload_json?: string;
};

export type ToolResult = unknown;

export type SessionRuntimeEvent =
  | { type: "assistant-part"; messageId: string; part: ChatPart }
  | { type: "tool-started"; toolCallId: string }
  | { type: "tool-result"; toolCallId: string; result: ToolResult }
  | { type: "usage"; messageId: string; usage: UsageSummary }
  | { type: "stream-completed" }
  | { type: "stream-failed"; error: Error };

const SESSION_MESSAGE_PART_TYPE = {
  TEXT: data.SessionMessagePartType?.TEXT ?? "SESSION_MESSAGE_PART_TYPE_TEXT",
  REASONING: data.SessionMessagePartType?.REASONING ?? "SESSION_MESSAGE_PART_TYPE_REASONING",
  TOOL_CALL: data.SessionMessagePartType?.TOOL_CALL ?? "SESSION_MESSAGE_PART_TYPE_TOOL_CALL",
  TOOL_RESULT: data.SessionMessagePartType?.TOOL_RESULT ?? "SESSION_MESSAGE_PART_TYPE_TOOL_RESULT",
  USAGE: data.SessionMessagePartType?.USAGE ?? "SESSION_MESSAGE_PART_TYPE_USAGE",
  ERROR: data.SessionMessagePartType?.ERROR ?? "SESSION_MESSAGE_PART_TYPE_ERROR",
  IMAGE: data.SessionMessagePartType?.IMAGE ?? "SESSION_MESSAGE_PART_TYPE_IMAGE",
};

function payload(value: unknown): Record<string, unknown> {
  if (typeof value !== "string" || value.length === 0) return {};
  try {
    const parsed = JSON.parse(value);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? parsed as Record<string, unknown>
      : {};
  } catch {
    return {};
  }
}

function toolCallId(part: ChatPart, parsedPayload: Record<string, unknown>): string {
  const direct = part.toolCallId ?? part.tool_call_id ?? part.id;
  if (typeof direct === "string" && direct.length > 0) return direct;
  const nested = parsedPayload.tool_call_id ?? parsedPayload.toolCallId;
  return typeof nested === "string" ? nested : "";
}

function eventKind(value: unknown): string | number | undefined {
  if (typeof value === "number" || typeof value === "string") return value;
  return undefined;
}

function isKind(value: unknown, numeric: number, named: string): boolean {
  return value === numeric || value === named;
}

export async function* streamSessionPartEvents(
  events: AsyncIterable<any>,
  signal?: AbortSignal,
): AsyncGenerator<SessionRuntimeEvent> {
  try {
    for await (const event of events) {
      if (signal?.aborted) return;
      const kind = eventKind(event?.kind);
      if (isKind(kind, 3, "SESSION_MESSAGE_PART_EVENT_KIND_ERROR")) {
        yield {
          type: "stream-failed",
          error: new Error(event?.part?.content || "Session stream error"),
        };
        return;
      }
      if (isKind(kind, 2, "SESSION_MESSAGE_PART_EVENT_KIND_DONE")) {
        yield { type: "stream-completed" };
        return;
      }

      const part = event?.part as ChatPart | undefined;
      if (!part) continue;
      const messageId = String(event?.messageId ?? event?.message_id ?? "");
      const partType = part.partType ?? part.part_type;
      const parsedPayload = payload(part.payloadJson ?? part.payload_json);

      if (partType === SESSION_MESSAGE_PART_TYPE.ERROR || partType === "SESSION_MESSAGE_PART_TYPE_ERROR") {
        yield { type: "stream-failed", error: new Error(String(part.content || "Session stream error")) };
        return;
      }

      yield { type: "assistant-part", messageId, part };

      if (partType === SESSION_MESSAGE_PART_TYPE.TOOL_CALL || partType === "SESSION_MESSAGE_PART_TYPE_TOOL_CALL") {
        const id = toolCallId(part, parsedPayload);
        if (id) yield { type: "tool-started", toolCallId: id };
      } else if (partType === SESSION_MESSAGE_PART_TYPE.TOOL_RESULT || partType === "SESSION_MESSAGE_PART_TYPE_TOOL_RESULT") {
        const id = toolCallId(part, parsedPayload);
        if (id) {
          yield {
            type: "tool-result",
            toolCallId: id,
            result: parsedPayload.output ?? parsedPayload.tool_output ?? parsedPayload.toolOutput ?? part.content,
          };
        }
      } else if (partType === SESSION_MESSAGE_PART_TYPE.USAGE || partType === "SESSION_MESSAGE_PART_TYPE_USAGE") {
        yield { type: "usage", messageId, usage: parsedPayload as UsageSummary };
      }
    }
    if (!signal?.aborted) yield { type: "stream-completed" };
  } catch (error) {
    if (!signal?.aborted) {
      yield { type: "stream-failed", error: error instanceof Error ? error : new Error(String(error)) };
    }
  }
}
