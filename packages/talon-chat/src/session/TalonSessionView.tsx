import React from "react";
import { ResourcePane, type ResourcePaneProps } from "../lib/ResourcePane";
import { SessionComposerDock, type SessionComposerDockProps } from "./SessionComposerDock";
import { SessionMessageList, type SessionMessageListProps } from "./SessionMessageList";
import { SessionStyles } from "./SessionStyles";
import { SessionTranscript, type SessionTranscriptProps } from "./SessionTranscript";

const talonChatFontFamily =
  'var(--talon-chat-font-family, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif)';

export type TalonSessionViewProps = {
  className?: string;
  composer: SessionComposerDockProps;
  messageList: SessionMessageListProps;
  resourcePane: ResourcePaneProps | null;
  style?: React.CSSProperties;
  transcript: Omit<SessionTranscriptProps, "children">;
};

/** Pure presentation shell for the transcript, composer, and optional resource pane. */
export function TalonSessionView({ className, composer, messageList, resourcePane, style, transcript }: TalonSessionViewProps) {
  return (
    <div
      className={className}
      style={{
        display: "flex",
        flexDirection: "column",
        minWidth: 0,
        minHeight: 0,
        height: "100%",
        background: "transparent",
        color: "inherit",
        fontFamily: talonChatFontFamily,
        ...style,
      }}
    >
      <SessionStyles />
      <div style={{ display: "flex", flexDirection: "row", flex: 1, minHeight: 0, minWidth: 0, position: "relative", overflow: "hidden" }}>
        <div style={{ display: "flex", flexDirection: "column", flex: "1 1 auto", minWidth: 0, minHeight: 0, transition: "flex 280ms cubic-bezier(0.22, 1, 0.36, 1)" }}>
          <SessionTranscript {...transcript}>
            <SessionMessageList {...messageList} />
          </SessionTranscript>
          <SessionComposerDock {...composer} />
        </div>
        {resourcePane ? <ResourcePane {...resourcePane} /> : null}
      </div>
    </div>
  );
}
