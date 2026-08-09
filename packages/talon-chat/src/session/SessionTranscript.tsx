import React from "react";
import { Activity } from "lucide-react";

export type SessionScrollThumb = { visible: boolean; top: number; height: number };

export type SessionTranscriptProps = {
  children: React.ReactNode;
  isLive: boolean;
  hasTrailingUserMessage: boolean;
  workingLabel?: string | null;
  error: Error | null;
  incident?: string | null;
  notice?: string | null;
  scrollThumb: SessionScrollThumb;
  transcriptRef: React.RefObject<HTMLDivElement | null>;
  bottomRef: React.RefObject<HTMLDivElement | null>;
  onScroll: () => void;
};

function border(color: string) {
  return `1px solid ${color}`;
}

export function SessionTranscript({
  children,
  isLive,
  hasTrailingUserMessage,
  workingLabel,
  error,
  incident,
  notice,
  scrollThumb,
  transcriptRef,
  bottomRef,
  onScroll,
}: SessionTranscriptProps) {
  return (
    <div style={{ position: "relative", flex: 1, minHeight: 0 }}>
      <div
        className="talon-session-transcript"
        data-testid="copilot-transcript"
        ref={transcriptRef}
        onScroll={onScroll}
        style={{ height: "100%", overflowY: "auto", overflowX: "hidden", minHeight: 0 }}
      >
        <div style={{ maxWidth: 896, margin: "0 auto", padding: "1.5rem", display: "flex", flexDirection: "column", gap: "2rem" }}>
          {children}
          {isLive && hasTrailingUserMessage ? (
            <div style={{ width: "100%" }}>
              <div style={{ fontSize: 13, fontWeight: 500, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>
                {workingLabel}
              </div>
            </div>
          ) : null}
          {error || incident ? (
            <div style={{ display: "flex", gap: "1rem" }}>
              <div style={{ flexShrink: 0 }}>
                <div style={{ width: 24, height: 24, borderRadius: 999, display: "flex", alignItems: "center", justifyContent: "center", background: "rgba(254,226,226,1)", border: border("rgba(252,165,165,1)") }}>
                  <Activity size="14" color="rgba(220,38,38,1)" strokeWidth={1.75} />
                </div>
              </div>
              <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 8 }}>
                <span style={{ fontSize: 13, fontWeight: 600, color: "rgba(220,38,38,1)" }}>Session Incident</span>
                <div style={{ fontSize: 13, borderRadius: 10, background: "rgba(254,242,242,1)", border: border("rgba(252,165,165,0.6)"), color: "rgba(220,38,38,1)", padding: 12, fontFamily: "ui-monospace, SFMono-Regular, monospace" }}>
                  {error?.message || incident || "An error occurred while connecting to the agent."}
                </div>
              </div>
            </div>
          ) : null}
          {notice ? (
            <div role="status" style={{ fontSize: 13, borderRadius: 10, background: "rgba(236,253,245,1)", border: border("rgba(110,231,183,0.8)"), color: "rgba(4,120,87,1)", padding: 12 }}>
              {notice}
            </div>
          ) : null}
          <div ref={bottomRef} />
        </div>
      </div>
      {scrollThumb.visible ? (
        <div aria-hidden="true" style={{ position: "absolute", top: scrollThumb.top, right: 2, width: 5, height: scrollThumb.height, borderRadius: 999, background: "var(--talon-chat-scrollbar-thumb, rgba(113,113,122,0.52))", pointerEvents: "none" }} />
      ) : null}
    </div>
  );
}
