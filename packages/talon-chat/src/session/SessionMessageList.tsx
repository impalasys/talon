import React from "react";
import { SessionMessage, type SessionMessageProps } from "./SessionMessage";
import type { CopilotMessage } from "../lib/chatTimeline";

type SessionMessageListProps = {
  messages: CopilotMessage[];
  messageProps: Omit<SessionMessageProps, "message" | "messageIndex" | "messages">;
};

/** Renders every transcript row through the single shared message presentation boundary. */
export function SessionMessageList({ messages, messageProps }: SessionMessageListProps) {
  return messages.map((message, messageIndex) => (
    <SessionMessage
      key={message.id}
      message={message}
      messageIndex={messageIndex}
      messages={messages}
      {...messageProps}
    />
  ));
}
