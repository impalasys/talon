import { TalonChatComposer, type TalonChatComposerProps } from "../lib/TalonChatComposer";
import { SessionPendingMessagesQueue } from "./SessionPendingMessagesQueue";
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
        <SessionPendingMessagesQueue messages={pendingMessages} />
        <TalonChatComposer {...props} />
      </div>
    </div>
  );
}
