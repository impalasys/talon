import type React from "react";
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
