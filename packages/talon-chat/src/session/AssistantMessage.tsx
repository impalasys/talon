import React from "react";
import { MarkdownMessage } from "../lib/MarkdownMessage";
import type { AssistantTimelineItem, CopilotMessage } from "../lib/chatTimeline";

export type AssistantMessageProps = {
  content: string;
  items: AssistantTimelineItem[];
  message: CopilotMessage;
  onResourceClick?: (uri: string) => void;
};

/** Renders the user-facing final response portion of an assistant message. */
export function AssistantMessage({ content, items, message, onResourceClick }: AssistantMessageProps) {
  const finalText = items
    .filter((item): item is Extract<AssistantTimelineItem, { type: "text" }> => item.type === "text")
    .map((item) => item.text)
    .join("");
  return (
    <div data-assistant-message-id={message.id} style={{ minWidth: 0, overflow: "hidden", overflowWrap: "anywhere", whiteSpace: "normal", fontSize: "var(--talon-chat-message-font-size, 1rem)", lineHeight: 1.65, opacity: 0.94 }}>
      <MarkdownMessage onResourceClick={onResourceClick}>{finalText || content}</MarkdownMessage>
    </div>
  );
}
