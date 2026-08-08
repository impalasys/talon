import React from "react";
import { ListOrdered } from "lucide-react";
import type { PendingSessionMessage } from "./useSessionPendingMessages";

export type SessionPendingMessagesQueueProps = {
  messages: PendingSessionMessage[];
};

/** Compact preview of messages waiting in the session's durable NEXT queue. */
export function SessionPendingMessagesQueue({ messages }: SessionPendingMessagesQueueProps) {
  if (messages.length === 0) return null;

  return (
    <div
      aria-label="Next queue"
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        margin: "0 12px 10px",
        padding: "9px 12px",
        border: "1px solid var(--talon-chat-border, rgba(15, 23, 42, 0.1))",
        borderRadius: 14,
        background: "var(--talon-chat-surface, rgba(255, 255, 255, 0.82))",
        color: "var(--talon-chat-muted-fg, rgba(71, 85, 105, 0.92))",
        boxShadow: "0 1px 3px rgba(15, 23, 42, 0.06)",
      }}
    >
      <ListOrdered size="16" aria-hidden="true" style={{ flexShrink: 0 }} />
      <span style={{ flexShrink: 0, fontSize: 12, fontWeight: 650 }}>Next</span>
      <span
        style={{
          flex: 1,
          minWidth: 0,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          fontSize: 13,
          color: "var(--talon-chat-user-bubble-fg, inherit)",
        }}
        title={messages.map(({ content }) => content).join("\n")}
      >
        {messages.map(({ entryId, content }, index) => (
          <React.Fragment key={entryId}>
            {index > 0 ? " · " : null}
            {content}
          </React.Fragment>
        ))}
      </span>
      <span style={{ flexShrink: 0, fontSize: 11, fontWeight: 600 }}>
        {messages.length} queued
      </span>
    </div>
  );
}
