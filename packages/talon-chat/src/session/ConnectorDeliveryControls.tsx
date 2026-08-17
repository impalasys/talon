import React from "react";
import type { CopilotMessage } from "../lib/chatTimeline";

export type ConnectorDeliveryControlsProps = {
  message: CopilotMessage;
  disabled: boolean;
  onUpdate: (message: CopilotMessage, status: string) => void;
};

export function ConnectorDeliveryControls({ message, disabled, onUpdate }: ConnectorDeliveryControlsProps) {
  const buttonStyle = (color: string, emphasized = false) => ({
    border: "none", background: "transparent", color, cursor: disabled ? "not-allowed" : "pointer",
    ...(emphasized ? { fontWeight: 700 } : {}), padding: "2px 4px", opacity: disabled ? 0.55 : 1,
  });
  return <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8, marginBottom: 8, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))", fontSize: 12 }}>
    <span>Pending send</span>
    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
      <button type="button" disabled={disabled} onClick={() => onUpdate(message, "delivery_requested")} style={buttonStyle("var(--talon-chat-accent-fg, #047857)", true)}>Send</button>
      <button type="button" disabled={disabled} onClick={() => onUpdate(message, "skipped")} style={buttonStyle("inherit")}>Skip</button>
    </div>
  </div>;
}
