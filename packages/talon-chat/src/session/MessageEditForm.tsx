import { Check, X } from "lucide-react";
import type { CopilotMessage } from "../lib/chatTimeline";

export type MessageEditFormProps = {
  message: CopilotMessage;
  value: string;
  onChange: (value: string) => void;
  onSave: (message: CopilotMessage) => void;
  onCancel: () => void;
};

function border(color: string) { return `1px solid ${color}`; }

export function MessageEditForm({ message, value, onChange, onSave, onCancel }: MessageEditFormProps) {
  const canSave = Boolean(value.trim());
  const buttonStyle = { width: 28, height: 28, display: "inline-flex", alignItems: "center", justifyContent: "center", borderRadius: 8, border: border("var(--talon-chat-edit-action-border, rgba(82,82,91,0.82))"), background: "var(--talon-chat-edit-action-bg, rgba(39,39,42,0.92))", color: "var(--talon-chat-edit-action-fg, inherit)" };
  return <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
    <textarea className="talon-session-edit-textarea" aria-label="Edit message" value={value} onChange={(event) => onChange(event.currentTarget.value)} rows={Math.min(8, Math.max(2, value.split("\n").length))} style={{ width: "100%", resize: "vertical", border: border("var(--talon-chat-edit-border, rgba(82,82,91,0.86))"), borderRadius: 8, background: "var(--talon-chat-edit-bg, rgba(24,24,27,0.92))", color: "var(--talon-chat-edit-fg, inherit)", padding: "0.65rem 0.8rem", font: "inherit", fontSize: "var(--talon-chat-message-font-size, 1rem)", lineHeight: 1.55, outline: "none", boxShadow: "var(--talon-chat-edit-shadow, inset 0 0 0 1px rgba(255,255,255,0.02))" }} />
    <div style={{ display: "flex", justifyContent: message.role === "user" ? "flex-end" : "flex-start", gap: 6 }}>
      <button className="talon-session-edit-action" type="button" aria-label="Save message edit" title="Save" onClick={() => onSave(message)} disabled={!canSave} style={{ ...buttonStyle, cursor: canSave ? "pointer" : "not-allowed", opacity: canSave ? 1 : 0.45 }}><Check size="14" strokeWidth={2} /></button>
      <button className="talon-session-edit-action" type="button" aria-label="Cancel message edit" title="Cancel" onClick={onCancel} style={{ ...buttonStyle, cursor: "pointer" }}><X size="14" strokeWidth={2} /></button>
    </div>
  </div>;
}
