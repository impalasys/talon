import React from "react";

export function SessionStyles() {
  return <style>{`
    .talon-session-tool-chevron { opacity: 0; transition: opacity 120ms ease, transform 160ms ease; }
    .talon-session-tool-row:hover .talon-session-tool-chevron,
    .talon-session-tool-row:focus-visible .talon-session-tool-chevron { opacity: 1; }
    .talon-session-message-actions { opacity: 0; pointer-events: none; transition: opacity 120ms ease; }
    .talon-session-message-row:hover .talon-session-message-actions { opacity: 1; pointer-events: auto; }
    .talon-session-message-action-button { transition: color 120ms ease, background 120ms ease; }
    .talon-session-message-action-button:hover,
    .talon-session-message-action-button:focus-visible {
      background: var(--talon-chat-edit-trigger-hover-bg, rgba(113,113,122,0.14)) !important;
      color: var(--talon-chat-edit-trigger-hover-fg, var(--talon-chat-edit-trigger-fg, inherit)) !important;
    }
    .talon-session-edit-textarea::placeholder { color: var(--talon-chat-edit-placeholder-fg, rgba(161,161,170,0.72)); }
    .talon-session-edit-textarea:focus {
      border-color: var(--talon-chat-edit-focus-border, rgba(161,161,170,0.95)) !important;
      box-shadow: var(--talon-chat-edit-focus-shadow, 0 0 0 2px rgba(161,161,170,0.2)) !important;
    }
    .talon-session-edit-action:hover:not(:disabled),
    .talon-session-edit-action:focus-visible:not(:disabled) { background: var(--talon-chat-edit-action-hover-bg, rgba(63,63,70,0.96)) !important; }
    .talon-session-transcript { scrollbar-width: none; }
    .talon-session-transcript::-webkit-scrollbar { display: none; width: 0; height: 0; }
  `}</style>;
}
