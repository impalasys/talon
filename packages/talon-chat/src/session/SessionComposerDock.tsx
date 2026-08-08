import React from "react";
import { ListOrdered } from "lucide-react";
import { TalonChatComposer, type TalonChatComposerProps } from "../lib/TalonChatComposer";
import type { PendingSessionMessage } from "./useSessionPendingMessages";

export type SessionComposerDockProps = TalonChatComposerProps & {
  disabled?: boolean;
  pendingMessages?: PendingSessionMessage[];
};

/** Presentation shell for the composer; behavior remains supplied by props. */
export function SessionComposerDock({ disabled, pendingMessages = [], ...props }: SessionComposerDockProps) {
  if (disabled) return null;
  return (
    <div style={{ position: "sticky", bottom: 0, zIndex: 10, flexShrink: 0, display: "flex", justifyContent: "center", width: "100%", boxSizing: "border-box", padding: "1.5rem", background: "var(--talon-chat-composer-bg, linear-gradient(to top, rgba(255,255,255,0.94), rgba(255,255,255,0.72) 58%, rgba(255,255,255,0)))", backdropFilter: "blur(10px)" }}>
      <div style={{ width: "100%", maxWidth: "var(--talon-chat-composer-max-width, 896px)", paddingBottom: 8 }}>
        {pendingMessages.length > 0 ? (
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
              title={pendingMessages.map(({ content }) => content).join("\n")}
            >
              {pendingMessages.map(({ entryId, content }, index) => (
                <React.Fragment key={entryId}>
                  {index > 0 ? " · " : null}
                  {content}
                </React.Fragment>
              ))}
            </span>
            <span style={{ flexShrink: 0, fontSize: 11, fontWeight: 600 }}>
              {pendingMessages.length} queued
            </span>
          </div>
        ) : null}
        <TalonChatComposer {...props} />
      </div>
    </div>
  );
}
