import { Copy, Pencil } from "lucide-react";
import type { CopilotMessage } from "../lib/chatTimeline";
import { historyMessageTimestamp } from "./history";

export type MessageActionsProps = {
  message: CopilotMessage;
  timestamp: string | null;
  onCopy: (message: CopilotMessage) => void;
  onEdit: (message: CopilotMessage) => void;
};

const buttonStyle = { width: 22, height: 22, display: "inline-flex", alignItems: "center", justifyContent: "center", borderRadius: 6, border: "none", background: "transparent", color: "var(--talon-chat-message-action-fg, var(--talon-chat-muted-fg, rgba(113,113,122,0.9)))", cursor: "pointer" };

export function MessageActions({ message, timestamp, onCopy, onEdit }: MessageActionsProps) {
  const isUser = message.role === "user";
  const timestampMs = historyMessageTimestamp(message);
  return <div className={`talon-session-message-actions${isUser ? " talon-session-message-actions-user" : ""}`} style={{ display: "flex", alignItems: "center", justifyContent: isUser ? "flex-end" : "flex-start", gap: 10, marginTop: 6, minHeight: 22 }}>
    {timestamp ? <span className="talon-session-message-action-time" title={new Date(timestampMs ?? 0).toLocaleString()} style={{ color: "var(--talon-chat-message-action-fg, var(--talon-chat-muted-fg, rgba(113,113,122,0.9)))", fontSize: 12, lineHeight: 1, whiteSpace: "nowrap" }}>{timestamp}</span> : null}
    <button className="talon-session-message-action-button" type="button" aria-label={`Copy ${message.role} message`} title="Copy" onClick={() => onCopy(message)} style={buttonStyle}><Copy size="14" strokeWidth={1.9} /></button>
    <button className="talon-session-edit-trigger talon-session-message-action-button" type="button" aria-label={`Edit ${message.role} message`} title="Edit" onClick={() => onEdit(message)} style={buttonStyle}><Pencil size="14" strokeWidth={1.9} /></button>
  </div>;
}
