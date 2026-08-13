import React from "react";
import { FileText, X } from "lucide-react";
import {
  formatArtifactBytes,
  formatArtifactCreatedAt,
  type SessionArtifact,
} from "./artifacts";

type SessionArtifactsRailProps = {
  artifacts: SessionArtifact[];
  error: Error | null;
  hasMore: boolean;
  isLoading: boolean;
  onLoadMore: () => void;
  onSelect: (artifact: SessionArtifact) => void;
  onDismiss: () => void;
};

function border(color: string) {
  return `1px solid ${color}`;
}

export function SessionArtifactsRail({
  artifacts,
  error,
  hasMore,
  isLoading,
  onLoadMore,
  onSelect,
  onDismiss,
}: SessionArtifactsRailProps) {
  return (
    <aside
      aria-label="Session artifacts"
      data-testid="session-artifacts-rail"
      className="talon-session-artifacts-rail"
      style={{
        position: "absolute",
        top: 16,
        right: 16,
        zIndex: 12,
        width: 304,
        maxHeight: "calc(100% - 32px)",
        minWidth: 0,
        minHeight: 0,
        overflow: "hidden",
        border: border("var(--talon-chat-divider, rgba(212,212,216,0.7))"),
        borderRadius: 18,
        background: "var(--talon-chat-resource-pane-bg, var(--talon-chat-composer-bg, rgba(255,255,255,0.96)))",
        boxShadow: "0 5px 16px rgba(24,24,27,0.06), 0 1px 3px rgba(24,24,27,0.04)",
        transition: "width 220ms cubic-bezier(0.22, 1, 0.36, 1), box-shadow 180ms ease",
      }}
    >
      <div style={{ width: 304, maxHeight: "calc(100vh - 32px)", display: "flex", flexDirection: "column" }}>
        <header style={{ minHeight: 48, padding: "0 0.85rem", display: "flex", alignItems: "center", borderBottom: border("var(--talon-chat-divider, rgba(212,212,216,0.7))") }}>
          <span style={{ minWidth: 0, fontSize: 14, fontWeight: 500, opacity: 0.64 }}>Artifacts</span>
          <button className="talon-session-artifacts-dismiss" type="button" aria-label="Close artifacts" onClick={onDismiss} style={{ marginLeft: "auto", border: "none", borderRadius: 6, background: "transparent", color: "inherit", cursor: "pointer", padding: 4 }}>
            <X size="18" strokeWidth={1.8} />
          </button>
        </header>

        <div style={{ minHeight: 0, maxHeight: "min(420px, calc(100vh - 96px))", overflowY: "auto", padding: "0.5rem" }}>
          {error ? <RailNotice tone="error">{error.message || "Could not load artifacts."}</RailNotice> : null}
          {artifacts.map((artifact) => {
            const details = [artifact.mediaType, formatArtifactBytes(artifact.sizeBytes), formatArtifactCreatedAt(artifact.createdAt)].filter(Boolean).join(" · ");
            return (
              <button key={artifact.id} type="button" onClick={() => onSelect(artifact)} style={{ width: "100%", border: "none", borderRadius: 10, background: "transparent", color: "inherit", cursor: "pointer", textAlign: "left", padding: "0.625rem", display: "flex", gap: 10, alignItems: "flex-start" }}>
                <FileText size="18" strokeWidth={1.7} style={{ marginTop: 1, flexShrink: 0, opacity: 0.48 }} />
                <span style={{ minWidth: 0, display: "flex", flexDirection: "column", gap: 3 }}>
                  <span title={artifact.title || artifact.id} style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 14, fontWeight: 500, lineHeight: 1.35, opacity: 0.64 }}>{artifact.title || artifact.id}</span>
                  {details ? <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 11, lineHeight: 1.35, opacity: 0.62 }}>{details}</span> : null}
                </span>
              </button>
            );
          })}
          {hasMore ? <button type="button" onClick={onLoadMore} disabled={isLoading} style={{ ...loadMoreStyle, opacity: isLoading ? 0.6 : 1 }}>{isLoading ? "Loading…" : "Load more"}</button> : null}
        </div>
      </div>
      <style>{`
        .talon-session-artifacts-rail button:not(:disabled):hover { background: var(--talon-chat-hover-bg, rgba(24, 24, 27, 0.06)); }
        .talon-session-artifacts-dismiss { display: none; }
        @media (max-width: 640px) {
          .talon-session-artifacts-rail { position: absolute !important; inset: 0 !important; z-index: 21; width: 100% !important; max-height: none !important; border-radius: 0 !important; box-shadow: none !important; }
          .talon-session-artifacts-dismiss { display: inline-flex; align-items: center; justify-content: center; }
        }
      `}</style>
    </aside>
  );
}

function RailNotice({ children, tone }: { children: React.ReactNode; tone?: "error" }) {
  return <div style={{ margin: "0.25rem", padding: "0.75rem", borderRadius: 10, color: tone === "error" ? "rgba(220,38,38,1)" : "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))", background: tone === "error" ? "rgba(254,242,242,1)" : "rgba(148,163,184,0.09)", fontSize: 12, lineHeight: 1.45 }}>{children}</div>;
}

const loadMoreStyle: React.CSSProperties = { width: "calc(100% - 0.5rem)", margin: "0.5rem 0.25rem", padding: "0.5rem", border: border("var(--talon-chat-divider, rgba(212,212,216,0.7))"), borderRadius: 8, background: "transparent", color: "inherit", fontSize: 12, cursor: "pointer" };
