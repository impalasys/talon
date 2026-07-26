"use client";

import React, { useEffect, useMemo, useState } from "react";
import { X } from "lucide-react";
import { MarkdownMessage } from "./MarkdownMessage";
import {
  parseResourceUri,
  type ResourceViewModel,
} from "./resourceUris";

function border(color: string) {
  return `1px solid ${color}`;
}

const MAX_INLINE_TEXT_BYTES = 3 * 1024 * 1024;
const PANE_TRANSITION_MS = 280;

function mediaTypeBase(mediaType: string): string {
  return mediaType.split(";")[0]?.trim().toLowerCase() || "";
}

function decodeContentAsText(content: Uint8Array | string | undefined): string | null {
  if (content == null) return null;
  if (typeof content === "string") return content;
  if (content.byteLength > MAX_INLINE_TEXT_BYTES) return null;
  try {
    return new TextDecoder("utf-8", { fatal: false }).decode(content);
  } catch {
    return null;
  }
}

function isMarkdownMediaType(mediaType: string): boolean {
  const base = mediaTypeBase(mediaType);
  return base === "text/markdown" || base === "text/x-markdown";
}

function isTextMediaType(mediaType: string): boolean {
  const base = mediaTypeBase(mediaType);
  return base.startsWith("text/") || base === "application/json" || base === "";
}

function isImageMediaType(mediaType: string): boolean {
  return mediaTypeBase(mediaType).startsWith("image/");
}

export type ResourcePaneProps = {
  uri: string;
  resource: ResourceViewModel | null;
  isLoading: boolean;
  error: Error | null;
  onClose: () => void;
  onResourceClick?: (uri: string) => void;
  /** When false, the pane animates closed before unmount (parent controls exit). */
  open?: boolean;
  /** Fired after the close transition finishes so the parent can unmount. */
  onExitComplete?: () => void;
};

export function ResourcePane({
  uri,
  resource,
  isLoading,
  error,
  onClose,
  onResourceClick,
  open = true,
  onExitComplete,
}: ResourcePaneProps) {
  const parsed = useMemo(() => parseResourceUri(uri), [uri]);
  const title =
    resource?.title ||
    (parsed?.kind === "artifact"
      ? parsed.artifactId
      : parsed?.kind === "file"
        ? parsed.fileName
        : uri);
  const mediaType = resource?.mediaType || "";

  const [blobUrl, setBlobUrl] = useState<string | null>(null);
  // Enter animation: mount closed, then flip open on the next frame.
  const [entered, setEntered] = useState(false);

  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      // Double rAF so the browser paints the closed state before transitioning.
      requestAnimationFrame(() => setEntered(true));
    });
    return () => cancelAnimationFrame(frame);
  }, []);

  useEffect(() => {
    if (open) return;
    const timer = window.setTimeout(() => {
      onExitComplete?.();
    }, PANE_TRANSITION_MS);
    return () => window.clearTimeout(timer);
  }, [open, onExitComplete]);

  useEffect(() => {
    // Build a blob URL for:
    // - image/* inline rendering when no signedUrl, and
    // - non-text/non-markdown binary downloads when content is only available as bytes.
    if (!resource || resource.signedUrl) {
      setBlobUrl(null);
      return;
    }
    if (!(resource.content instanceof Uint8Array) || resource.content.byteLength === 0) {
      setBlobUrl(null);
      return;
    }
    const isImage = isImageMediaType(resource.mediaType);
    const isInlineText =
      isTextMediaType(resource.mediaType) || isMarkdownMediaType(resource.mediaType);
    if (!isImage && isInlineText) {
      setBlobUrl(null);
      return;
    }
    const bytes = resource.content;
    const copy = new Uint8Array(bytes.byteLength);
    copy.set(bytes);
    const blob = new Blob([copy.buffer], {
      type: mediaTypeBase(resource.mediaType) || "application/octet-stream",
    });
    const url = URL.createObjectURL(blob);
    setBlobUrl(url);
    return () => {
      URL.revokeObjectURL(url);
    };
  }, [resource]);

  const textContent = useMemo(() => {
    if (!resource) return null;
    if (!isTextMediaType(resource.mediaType) && !isMarkdownMediaType(resource.mediaType)) {
      return null;
    }
    return decodeContentAsText(resource.content);
  }, [resource]);

  const imageSrc = resource?.signedUrl || blobUrl || null;
  const downloadHref = resource?.signedUrl || blobUrl || null;
  const isVisible = open && entered;

  return (
    <aside
      className="talon-resource-pane"
      data-testid="talon-resource-pane"
      data-open={isVisible ? "true" : "false"}
      style={{
        display: "flex",
        flexDirection: "column",
        // Width-driven split animates more reliably than flex-basis alone.
        flex: "0 0 auto",
        width: isVisible ? "50%" : "0%",
        minWidth: 0,
        minHeight: 0,
        height: "100%",
        overflow: "hidden",
        borderLeft: isVisible
          ? border("var(--talon-chat-divider, rgba(212,212,216,0.7))")
          : "1px solid transparent",
        background: "var(--talon-chat-resource-pane-bg, var(--talon-chat-composer-bg, transparent))",
        boxSizing: "border-box",
        opacity: isVisible ? 1 : 0,
        transform: isVisible ? "translateX(0)" : "translateX(1rem)",
        transition: [
          `width ${PANE_TRANSITION_MS}ms cubic-bezier(0.22, 1, 0.36, 1)`,
          `opacity ${Math.round(PANE_TRANSITION_MS * 0.85)}ms ease`,
          `transform ${PANE_TRANSITION_MS}ms cubic-bezier(0.22, 1, 0.36, 1)`,
          `border-color ${PANE_TRANSITION_MS}ms ease`,
        ].join(", "),
        willChange: "width, opacity, transform",
      }}
    >
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          // Keep pane content at a stable width during the open animation so
          // text doesn't reflow as the shell expands.
          width: "100%",
          minWidth: "min(100%, 18rem)",
          height: "100%",
          minHeight: 0,
          opacity: isVisible ? 1 : 0.4,
          transition: `opacity ${Math.round(PANE_TRANSITION_MS * 0.7)}ms ease`,
        }}
      >
        <header
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "0.85rem 1rem",
            borderBottom: border("var(--talon-chat-divider, rgba(212,212,216,0.7))"),
            flexShrink: 0,
          }}
        >
          <div
            style={{
              flex: 1,
              minWidth: 0,
              fontSize: 14,
              fontWeight: 600,
              lineHeight: 1.35,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
            title={title}
          >
            {title}
          </div>
          <button
            type="button"
            aria-label="Close resource pane"
            onClick={onClose}
            style={{
              border: "none",
              background: "transparent",
              cursor: "pointer",
              padding: 4,
              borderRadius: 6,
              color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))",
              flexShrink: 0,
            }}
          >
            <X size="16" strokeWidth={1.9} />
          </button>
        </header>

        <div
          style={{
            flex: 1,
            minHeight: 0,
            overflowY: "auto",
            overflowX: "hidden",
            padding: "1rem",
            fontSize: 13,
            lineHeight: 1.55,
          }}
        >
          {isLoading ? (
            <div style={{ color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>Loading…</div>
          ) : null}

          {!isLoading && error ? (
            <div
              style={{
                borderRadius: 10,
                border: border("rgba(252,165,165,0.6)"),
                background: "rgba(254,242,242,1)",
                color: "rgba(220,38,38,1)",
                padding: 12,
                fontSize: 13,
              }}
            >
              {formatResourceError(error)}
            </div>
          ) : null}

          {!isLoading && !error && resource ? (
            isMarkdownMediaType(resource.mediaType) && textContent != null ? (
              <MarkdownMessage onResourceClick={onResourceClick}>{textContent}</MarkdownMessage>
            ) : isTextMediaType(resource.mediaType) && textContent != null ? (
              <pre
                style={{
                  margin: 0,
                  padding: "0.75rem",
                  overflowX: "auto",
                  borderRadius: 12,
                  border: border("rgba(148,163,184,0.24)"),
                  background: "var(--talon-chat-code-bg, rgba(24,24,27,0.05))",
                  fontFamily: "ui-monospace, SFMono-Regular, monospace",
                  fontSize: 12,
                  whiteSpace: "pre-wrap",
                  overflowWrap: "anywhere",
                }}
              >
                {textContent}
              </pre>
            ) : isImageMediaType(resource.mediaType) && imageSrc ? (
              <img
                src={imageSrc}
                alt={title}
                style={{
                  maxWidth: "100%",
                  height: "auto",
                  borderRadius: 8,
                  border: border("rgba(148,163,184,0.24)"),
                }}
              />
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                <div style={{ color: "var(--talon-chat-subtle-fg, rgba(82,82,91,0.96))" }}>
                  This resource is not shown inline
                  {mediaType ? ` (${mediaType})` : ""}.
                </div>
                {downloadHref ? (
                  <a
                    href={downloadHref}
                    target="_blank"
                    rel="noreferrer"
                    download={title}
                    style={{
                      color: "var(--talon-chat-link-fg, var(--talon-chat-accent-fg, #047857))",
                      textDecoration: "underline",
                      fontWeight: 600,
                    }}
                  >
                    Download
                  </a>
                ) : null}
              </div>
            )
          ) : null}
        </div>
      </div>

      <style>
        {`
          @media (max-width: 640px) {
            .talon-resource-pane {
              position: absolute !important;
              inset: 0 !important;
              width: 100% !important;
              max-width: none !important;
              flex: 1 1 auto !important;
              border-left: none !important;
              z-index: 20;
              background: var(--talon-chat-resource-pane-bg, #fff);
              transform: none !important;
            }
            .talon-resource-pane[data-open="false"] {
              opacity: 0 !important;
              pointer-events: none;
            }
          }
        `}
      </style>
    </aside>
  );
}

function formatResourceError(error: Error): string {
  const message = error.message || "Failed to load resource";
  const lower = message.toLowerCase();
  if (lower.includes("permission") || lower.includes("denied") || lower.includes("403")) {
    return "Access denied";
  }
  if (lower.includes("not found") || lower.includes("404")) {
    return "Not found";
  }
  return message;
}
