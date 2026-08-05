import type React from "react";

export type SessionMessageProps = {
  messageId: string;
  children: React.ReactNode;
};

/** Stable presentation boundary for one transcript message. */
export function SessionMessage({ messageId, children }: SessionMessageProps) {
  return <div data-session-message-id={messageId}>{children}</div>;
}
