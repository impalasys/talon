import { getMessageAssistantTimeline, getMessageContent, type AssistantTimelineItem, type CopilotMessage } from "../lib/chatTimeline";
import {
  coalesceAssistantMessageTimelineForDisplay,
  splitAssistantMessageTimeline,
} from "./AssistantMessageTimeline";
import { SESSION_MESSAGE_PART_TYPE, isSessionTextPart } from "./protocol";

export function replaceMessageTextPart(message: CopilotMessage, text: string) {
  const parts = (Array.isArray(message.parts) ? message.parts : []).map((part: any) =>
    part && typeof part === "object" ? { ...part } : part,
  );
  const index = parts.findLastIndex((part) => isSessionTextPart(part));
  if (index >= 0) {
    const part = parts[index] as any;
    if ("text" in part) part.text = text;
    else part.content = text;
    return parts;
  }
  return [...parts, { partType: SESSION_MESSAGE_PART_TYPE.TEXT, content: text }];
}

export function messageWithEditedContent(message: CopilotMessage, content: string): CopilotMessage {
  const next = { ...message, content };
  if (Array.isArray(next.parts)) {
    let replaced = false;
    next.parts = next.parts.map((part: any) => {
      const type = part?.partType ?? part?.part_type ?? part?.type;
      const isText = type === "text" || type === SESSION_MESSAGE_PART_TYPE.TEXT || type === "SESSION_MESSAGE_PART_TYPE_TEXT";
      if (!part || typeof part !== "object" || !isText || replaced) return part;
      replaced = true;
      return { ...part, ...("text" in part ? { text: content } : {}), ...("content" in part ? { content } : {}) };
    });
  }
  if (message.role === "assistant") next.timeline = [{ type: "text", text: content }];
  return next;
}

export function editableMessageContent(message: CopilotMessage) {
  if (message.role !== "assistant") return getMessageContent(message);
  const timeline = coalesceAssistantMessageTimelineForDisplay(getMessageAssistantTimeline(message));
  const { finalTimeline } = splitAssistantMessageTimeline(timeline);
  const visible = finalTimeline.length > 0 ? finalTimeline : timeline;
  const text = visible.filter((item): item is Extract<AssistantTimelineItem, { type: "text" }> => item.type === "text");
  return text.length > 0 ? text.map((item) => item.text).join("") : getMessageContent(message);
}
