import type React from "react";
import { data } from "@impalasys/talon-client";
import {
  appendAssistantReasoning,
  appendAssistantText,
  applyToolInvocationToMessages,
  applyUsageToMessages,
  ensureAssistantMessage,
  formatUsageSummary,
  reconcileAssistantMessageId,
  type CopilotMessage,
  type UsageSummary,
} from "./chatTimeline";

export type StreamEventItem = {
  type: "status" | "tool_call" | "tool_result" | "reasoning" | "usage" | "error";
  content: string;
  name?: string;
  payload?: unknown;
};

const UI_STREAM_ASSISTANT_MESSAGE_ID_CODE = "f";
const UI_STREAM_TEXT_CHUNK_CODE = "0";
const UI_STREAM_REASONING_CHUNK_CODE = "g";
const UI_STREAM_TOOL_CALL_CODE = "9";
const UI_STREAM_TOOL_RESULT_CODE = "a";
const UI_STREAM_USAGE_CODE = "h";
const UI_STREAM_ERROR_CODE = "3";

function createLocalMessageId() {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return `local-${crypto.randomUUID()}`;
  }
  return `local-${Math.random().toString(36).slice(2, 11)}`;
}

function appendAssistantPart(messages: CopilotMessage[], messageId: string, part: unknown): CopilotMessage[] {
  return messages.map((message) => {
    if (message.id !== messageId) return message;
    const parts = Array.isArray(message.parts) ? [...message.parts, part] : [part];
    return { ...message, parts };
  });
}

export function sessionResponseHasAssistantText(response: any): boolean {
  return Array.isArray(response?.messages) && response.messages.some((message: any) => {
    return (
      (message?.role === 2 || message?.role === "ROLE_ASSISTANT" || message?.role === "assistant") &&
      typeof message?.content === "string" &&
      message.content.trim().length > 0
    );
  });
}

export async function streamSessionResume(options: {
  response: Response;
  setMessages: React.Dispatch<React.SetStateAction<CopilotMessage[]>>;
  setError: React.Dispatch<React.SetStateAction<Error | null>>;
  signal?: AbortSignal;
}) {
  const { response, setMessages, setError, signal } = options;
  if (!response.body) return;

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  while (true) {
    if (signal?.aborted) {
      await reader.cancel().catch(() => undefined);
      break;
    }
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });

    while (true) {
      const newlineIndex = buffer.indexOf("\n");
      if (newlineIndex < 0) break;

      const line = buffer.slice(0, newlineIndex);
      buffer = buffer.slice(newlineIndex + 1);
      if (!line) continue;

      let part: any;
      try {
        part = JSON.parse(line);
      } catch {
        continue;
      }

      if (signal?.aborted) {
        await reader.cancel().catch(() => undefined);
        return;
      }

      if (part.type === "text") {
        setMessages((prev) => {
          const lastAssistant = [...prev].reverse().find((message) => message.role === "assistant");
          return lastAssistant?.id ? appendAssistantText(prev, lastAssistant.id, String(part.value ?? "")) : prev;
        });
      } else if (part.type === "reasoning") {
        setMessages((prev) => {
          const lastAssistant = [...prev].reverse().find((message) => message.role === "assistant");
          return lastAssistant?.id
            ? appendAssistantReasoning(prev, lastAssistant.id, String(part.value ?? ""))
            : prev;
        });
      } else if (part.type === "tool_call") {
        setMessages((prev) =>
          applyToolInvocationToMessages(prev, part.value?.toolCallId, part.value?.toolName, part.value?.args),
        );
      } else if (part.type === "tool_result") {
        setMessages((prev) =>
          applyToolInvocationToMessages(
            prev,
            part.value?.toolCallId,
            "",
            undefined,
            part.value?.result,
          ),
        );
      } else if (part.type === "usage") {
        setMessages((prev) => {
          const lastAssistant = [...prev].reverse().find((message) => message.role === "assistant");
          return lastAssistant?.id ? applyUsageToMessages(prev, lastAssistant.id, part.value) : prev;
        });
      } else if (part.type === "error") {
        if (!signal?.aborted) {
          setError(new Error(String(part.value)));
        }
      }
    }
  }
}

export async function streamUiSubmission(options: {
  response: Response;
  setMessages: React.Dispatch<React.SetStateAction<CopilotMessage[]>>;
  setStreamEvents: React.Dispatch<React.SetStateAction<StreamEventItem[]>>;
}) {
  const { response, setMessages, setStreamEvents } = options;
  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error("Gateway response body was empty.");
  }

  const decoder = new TextDecoder();
  let buffer = "";
  let assistantText = "";
  let assistantMessageId: string | null = null;

  const ensureLiveAssistant = (messageId?: string) => {
    const nextMessageId = messageId || assistantMessageId || createLocalMessageId();
    const previousMessageId = assistantMessageId;
    const idChanged = Boolean(previousMessageId && previousMessageId !== nextMessageId);
    const isNew = !previousMessageId;
    assistantMessageId = nextMessageId;
    if (idChanged || isNew) {
      setMessages((prev) => {
        const reconciled =
          previousMessageId && previousMessageId !== nextMessageId
            ? reconcileAssistantMessageId(prev, previousMessageId, nextMessageId)
            : prev;
        return ensureAssistantMessage(reconciled, nextMessageId);
      });
    }
    return nextMessageId;
  };

  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });

    while (true) {
      const newlineIndex = buffer.indexOf("\n");
      if (newlineIndex < 0) break;

      const line = buffer.slice(0, newlineIndex);
      buffer = buffer.slice(newlineIndex + 1);
      if (!line) continue;

      const separatorIndex = line.indexOf(":");
      if (separatorIndex < 0) continue;

      const code = line.slice(0, separatorIndex);
      let part: any;
      try {
        part = JSON.parse(line.slice(separatorIndex + 1));
      } catch {
        continue;
      }

      if (code === UI_STREAM_ASSISTANT_MESSAGE_ID_CODE && typeof part?.messageId === "string" && part.messageId) {
        ensureLiveAssistant(part.messageId);
      } else if (code === UI_STREAM_TEXT_CHUNK_CODE && typeof part === "string") {
        const messageId = ensureLiveAssistant();
        assistantText += part;
        setMessages((prev) => appendAssistantText(prev, messageId, part));
      } else if (code === UI_STREAM_REASONING_CHUNK_CODE && typeof part === "string") {
        const messageId = ensureLiveAssistant();
        setStreamEvents((prev) => [...prev, { type: "reasoning", content: part }]);
        setMessages((prev) => appendAssistantReasoning(prev, messageId, part));
      } else if (code === UI_STREAM_TOOL_CALL_CODE) {
        const messageId = ensureLiveAssistant();
        setStreamEvents((prev) => [
          ...prev,
          {
            type: "tool_call",
            content: typeof part?.toolName === "string" ? part.toolName : "tool",
            name: part?.toolName,
            payload: part,
          },
        ]);
        setMessages((prev) =>
          applyToolInvocationToMessages(
            prev,
            typeof part?.toolCallId === "string" ? part.toolCallId : `tool-${createLocalMessageId()}`,
            typeof part?.toolName === "string" ? part.toolName : "tool",
            part?.args,
            undefined,
            messageId,
          ),
        );
      } else if (code === UI_STREAM_TOOL_RESULT_CODE) {
        const messageId = ensureLiveAssistant();
        setStreamEvents((prev) => [
          ...prev,
          {
            type: "tool_result",
            content: typeof part?.toolCallId === "string" ? part.toolCallId : "tool_result",
            payload: part,
          },
        ]);
        setMessages((prev) =>
          applyToolInvocationToMessages(
            prev,
            typeof part?.toolCallId === "string" ? part.toolCallId : `tool-${createLocalMessageId()}`,
            "",
            undefined,
            part?.result,
            messageId,
          ),
        );
      } else if (code === UI_STREAM_USAGE_CODE && part && typeof part === "object") {
        const messageId = ensureLiveAssistant();
        setStreamEvents((prev) => [
          ...prev,
          {
            type: "usage",
            content: formatUsageSummary(part as UsageSummary),
            payload: part,
          },
        ]);
        setMessages((prev) => applyUsageToMessages(prev, messageId, part as UsageSummary));
      } else if (code === UI_STREAM_ERROR_CODE) {
        throw new Error(typeof part === "string" ? part : "Stream error");
      }
    }
  }

  return { assistantText };
}

export async function streamSessionPartEvents(options: {
  events: AsyncIterable<any>;
  setMessages: React.Dispatch<React.SetStateAction<CopilotMessage[]>>;
  setStreamEvents: React.Dispatch<React.SetStateAction<StreamEventItem[]>>;
  signal?: AbortSignal;
}) {
  const { events, setMessages, setStreamEvents, signal } = options;
  let assistantText = "";
  let assistantMessageId: string | null = null;

  const ensureLiveAssistant = (messageId?: string) => {
    const nextMessageId = messageId || assistantMessageId || createLocalMessageId();
    const previousMessageId = assistantMessageId;
    const idChanged = Boolean(previousMessageId && previousMessageId !== nextMessageId);
    const isNew = !previousMessageId;
    assistantMessageId = nextMessageId;
    if (idChanged || isNew) {
      setMessages((prev) => {
        const reconciled =
          previousMessageId && previousMessageId !== nextMessageId
            ? reconcileAssistantMessageId(prev, previousMessageId, nextMessageId)
            : prev;
        return ensureAssistantMessage(reconciled, nextMessageId);
      });
    }
    return nextMessageId;
  };

  for await (const event of events) {
    if (signal?.aborted) break;
    const kind = event?.kind;
    if (kind === 3 || kind === "SESSION_MESSAGE_PART_EVENT_KIND_ERROR") {
      const error = new Error(event?.part?.content || "Session stream error");
      throw error;
    }
    if (kind === 2 || kind === "SESSION_MESSAGE_PART_EVENT_KIND_DONE") {
      break;
    }

    const part = event?.part;
    if (!part) continue;
    const partType = part.partType ?? part.part_type;
    const content = String(part.content ?? "");
    const payload = parsePayload(part.payloadJson ?? part.payload_json);
    const messageId = ensureLiveAssistant(event?.messageId ?? event?.message_id);

    if (partType === data.SessionMessagePartType.TEXT || partType === "SESSION_MESSAGE_PART_TYPE_TEXT") {
      assistantText += content;
      setMessages((prev) => appendAssistantText(prev, messageId, content));
    } else if (partType === data.SessionMessagePartType.REASONING || partType === "SESSION_MESSAGE_PART_TYPE_REASONING") {
      setStreamEvents((prev) => [...prev, { type: "reasoning", content }]);
      setMessages((prev) => appendAssistantReasoning(prev, messageId, content));
    } else if (partType === data.SessionMessagePartType.TOOL_CALL || partType === "SESSION_MESSAGE_PART_TYPE_TOOL_CALL") {
      const toolCallId = typeof payload?.tool_call_id === "string" ? payload.tool_call_id : part.id || `tool-${createLocalMessageId()}`;
      const toolName = typeof part.name === "string" && part.name ? part.name : "tool";
      setStreamEvents((prev) => [...prev, { type: "tool_call", content: toolName, name: toolName, payload }]);
      setMessages((prev) => applyToolInvocationToMessages(prev, toolCallId, toolName, payload?.input, undefined, messageId));
    } else if (partType === data.SessionMessagePartType.TOOL_RESULT || partType === "SESSION_MESSAGE_PART_TYPE_TOOL_RESULT") {
      const toolCallId = typeof payload?.tool_call_id === "string" ? payload.tool_call_id : part.id || `tool-${createLocalMessageId()}`;
      setStreamEvents((prev) => [...prev, { type: "tool_result", content: toolCallId, payload }]);
      setMessages((prev) => applyToolInvocationToMessages(prev, toolCallId, "", undefined, payload?.output ?? content, messageId));
    } else if (partType === data.SessionMessagePartType.USAGE || partType === "SESSION_MESSAGE_PART_TYPE_USAGE") {
      const usage = payload && typeof payload === "object" ? payload as UsageSummary : {};
      setStreamEvents((prev) => [...prev, { type: "usage", content: formatUsageSummary(usage), payload: usage }]);
      setMessages((prev) => applyUsageToMessages(prev, messageId, usage));
    } else if (partType === data.SessionMessagePartType.ERROR || partType === "SESSION_MESSAGE_PART_TYPE_ERROR") {
      const error = new Error(content || "Session stream error");
      throw error;
    } else if (partType === data.SessionMessagePartType.IMAGE || partType === "SESSION_MESSAGE_PART_TYPE_IMAGE") {
      setMessages((prev) => appendAssistantPart(prev, messageId, part));
    }
  }

  return { assistantText };
}

function parsePayload(value: unknown): any {
  if (typeof value !== "string" || !value) return undefined;
  try {
    return JSON.parse(value);
  } catch {
    return undefined;
  }
}
