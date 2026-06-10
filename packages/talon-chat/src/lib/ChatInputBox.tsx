import React, { useCallback, useMemo, useRef, useState } from "react";
import {
  ArrowUp,
  ChevronRight,
  Ellipsis,
  FileText,
  Globe,
  ImagePlus,
  Paperclip,
  Plus,
  Square,
  Telescope,
  Terminal,
  X,
} from "lucide-react";

function border(color: string) {
  return `1px solid ${color}`;
}

const talonChatFontFamily =
  'var(--talon-chat-font-family, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif)';

export type ChatInputBoxProps = {
  value: string;
  onValueChange: (value: string) => void;
  onSubmit: (value: string) => void;
  placeholder: string;
  autoFocus?: boolean;
  disabled?: boolean;
  rows?: number;
  canSubmit?: boolean;
  isGenerating?: boolean;
  canStop?: boolean;
  onStop?: () => void;
  helperText?: string;
  submitLabel?: string;
  stopLabel?: string;
  textareaMinHeight?: number;
  textareaMaxHeight?: number | string;
  commandMenuItems?: ChatInputCommandMenuItem[];
  imageAttachments?: ChatInputImageAttachment[];
  imageUploadEnabled?: boolean;
  imageAccept?: string;
  imageButtonLabel?: string;
  onImageFilesSelected?: (files: File[]) => void;
  onRemoveImageAttachment?: (id: string) => void;
  style?: React.CSSProperties;
};

export type ChatInputCommandMenuItem = {
  name: string;
  aliases?: string[];
  description?: string;
};

export type ChatInputImageAttachment = {
  id: string;
  filename: string;
  previewUrl: string;
  status?: "queued" | "uploading" | "ready" | "error";
  error?: string;
};

export function ChatInputBox({
  value,
  onValueChange,
  onSubmit,
  placeholder,
  autoFocus = false,
  disabled = false,
  rows = 1,
  canSubmit,
  isGenerating = false,
  canStop = true,
  onStop,
  helperText,
  submitLabel = "Send message",
  stopLabel = "Stop generation",
  textareaMinHeight = 24,
  textareaMaxHeight = "40vh",
  commandMenuItems,
  imageAttachments,
  imageUploadEnabled = false,
  imageAccept = "image/png,image/jpeg,image/gif,image/webp",
  imageButtonLabel = "Add photos & files",
  onImageFilesSelected,
  onRemoveImageAttachment,
  style,
}: ChatInputBoxProps) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const [highlightedCommandIndex, setHighlightedCommandIndex] = useState(0);
  const [hoveredCommandIndex, setHoveredCommandIndex] = useState<number | null>(null);
  const [showAttachmentMenu, setShowAttachmentMenu] = useState(false);
  const [hoveredAttachmentIndex, setHoveredAttachmentIndex] = useState<number | null>(null);
  const resolvedCanSubmit = canSubmit ?? (Boolean(value.trim()) && !disabled && !isGenerating);
  const isStopMode = Boolean(isGenerating && onStop);
  const resolvedCanStop = Boolean(isStopMode && canStop);
  const buttonDisabled = isStopMode ? !resolvedCanStop : !resolvedCanSubmit;
  const buttonIsEnabled = !buttonDisabled;
  const buttonSize = 34;
  const isSingleLine = rows <= 1;
  const attachments = imageAttachments ?? [];
  const textareaLineHeight = isSingleLine ? buttonSize : 20;
  const resolvedTextareaMinHeight = rows > 1 ? textareaMinHeight : buttonSize;
  const commandQuery = value.trimStart().startsWith("/") ? value.trimStart().slice(1).toLowerCase() : null;
  const visibleCommandItems = useMemo(() => {
    if (commandQuery === null || !commandMenuItems?.length || isGenerating) return [];
    return commandMenuItems.filter((item) => {
      const normalizedName = item.name.toLowerCase();
      if (normalizedName.startsWith(commandQuery)) return true;
      return item.aliases?.some((alias) => alias.toLowerCase().startsWith(commandQuery)) ?? false;
    });
  }, [commandMenuItems, commandQuery, isGenerating]);
  const shouldShowCommandMenu = visibleCommandItems.length > 0 && !disabled;
  const highlightedCommand = visibleCommandItems[Math.min(highlightedCommandIndex, visibleCommandItems.length - 1)];

  const submitValue = useCallback(() => {
    if (!resolvedCanSubmit) return;
    setShowAttachmentMenu(false);
    onSubmit(value);
  }, [onSubmit, resolvedCanSubmit, value]);

  const selectCommand = useCallback((item: ChatInputCommandMenuItem) => {
    onValueChange(`/${item.name}`);
    window.requestAnimationFrame(() => {
      textareaRef.current?.focus();
    });
  }, [onValueChange]);

  return (
    <>
      <style>
        {`
          .talon-chat-input-textarea::placeholder {
            color: var(--copilot-input-placeholder, rgba(82,82,91,0.72));
            opacity: 1;
          }
        `}
      </style>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          submitValue();
        }}
        style={{
          position: "relative",
          display: "flex",
          alignItems: "flex-end",
          gap: 8,
          width: "100%",
          boxSizing: "border-box",
          borderRadius: 18,
          border: border("var(--copilot-input-border, rgba(212,212,216,0.72))"),
          background: "var(--copilot-input-bg, rgba(255,255,255,0.96))",
          boxShadow: "var(--copilot-input-shadow, 0 4px 14px rgba(24,24,27,0.08), 0 1px 2px rgba(24,24,27,0.06))",
          padding: "0.25rem 0.3125rem 0.25rem 0.625rem",
          backdropFilter: "blur(12px)",
          flexWrap: attachments.length > 0 ? "wrap" : "nowrap",
          fontFamily: talonChatFontFamily,
          ...style,
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            setShowAttachmentMenu(false);
          }
        }}
      >
        {shouldShowCommandMenu ? (
          <div
            role="listbox"
            aria-label="Command menu"
            style={{
              position: "absolute",
              left: 0,
              right: 0,
              bottom: "calc(100% + 8px)",
              zIndex: 20,
              overflow: "hidden",
              borderRadius: 12,
              border: border("var(--copilot-command-menu-border, rgba(212,212,216,0.84))"),
              background: "var(--copilot-command-menu-bg, rgba(255,255,255,0.98))",
              boxShadow: "var(--copilot-command-menu-shadow, 0 14px 32px rgba(24,24,27,0.14), 0 2px 8px rgba(24,24,27,0.08))",
              color: "inherit",
            }}
          >
            {visibleCommandItems.map((item, index) => {
              const isHighlighted = index === Math.min(highlightedCommandIndex, visibleCommandItems.length - 1);
              const isHovered = hoveredCommandIndex === index;
              return (
                <button
                  key={item.name}
                  type="button"
                  role="option"
                  aria-selected={isHighlighted}
                  onMouseEnter={() => {
                    setHighlightedCommandIndex(index);
                    setHoveredCommandIndex(index);
                  }}
                  onMouseLeave={() => setHoveredCommandIndex(null)}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => selectCommand(item)}
                  style={{
                    width: "100%",
                    boxSizing: "border-box",
                    border: "none",
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "0.75rem 0.875rem",
                    background: isHovered
                      ? "var(--copilot-command-menu-hover-bg, rgba(24,24,27,0.11))"
                      : isHighlighted
                        ? "var(--copilot-command-menu-active-bg, rgba(24,24,27,0.06))"
                        : "transparent",
                    boxShadow: isHovered ? "inset 0 0 0 1px var(--copilot-command-menu-hover-border, rgba(24,24,27,0.10))" : "none",
                    color: "inherit",
                    cursor: "pointer",
                    textAlign: "left",
                    fontFamily: "inherit",
                    transition: "background 120ms ease, box-shadow 120ms ease",
                  }}
                >
                  <span
                    aria-hidden="true"
                    style={{
                      width: 28,
                      height: 28,
                      flexShrink: 0,
                      borderRadius: 8,
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      background: isHovered
                        ? "var(--copilot-command-menu-icon-hover-bg, rgba(24,24,27,0.16))"
                        : "var(--copilot-command-menu-icon-bg, rgba(24,24,27,0.08))",
                      color: isHovered
                        ? "var(--copilot-command-menu-icon-hover-fg, rgba(24,24,27,0.96))"
                        : "var(--copilot-command-menu-icon-fg, rgba(39,39,42,0.86))",
                      transition: "background 120ms ease, color 120ms ease",
                    }}
                  >
                    <Terminal size="15" strokeWidth={2} />
                  </span>
                  <span style={{ minWidth: 0, display: "flex", flexDirection: "column", gap: 2 }}>
                    <span style={{ fontSize: 14, fontWeight: 650, lineHeight: 1.2 }}>/{item.name}</span>
                    {item.description ? (
                      <span style={{ fontSize: 12, lineHeight: 1.35, opacity: 0.68, overflowWrap: "anywhere" }}>
                        {item.description}
                      </span>
                    ) : null}
                  </span>
                </button>
              );
            })}
          </div>
        ) : null}
        {attachments.length > 0 ? (
          <div
            style={{
              flexBasis: "100%",
              display: "flex",
              gap: 8,
              overflowX: "auto",
              padding: "0.25rem 0.25rem 0.125rem 0",
            }}
          >
            {attachments.map((attachment) => (
              <div
                key={attachment.id}
                style={{
                  position: "relative",
                  width: 58,
                  height: 58,
                  flexShrink: 0,
                  overflow: "hidden",
                  borderRadius: 8,
                  border: border(
                    attachment.status === "error"
                      ? "var(--copilot-attachment-error-border, rgba(220,38,38,0.55))"
                      : "var(--copilot-attachment-border, rgba(212,212,216,0.9))",
                  ),
                  background: "var(--copilot-attachment-bg, rgba(244,244,245,0.88))",
                }}
                title={attachment.error || attachment.filename}
              >
                <img
                  src={attachment.previewUrl}
                  alt={attachment.filename}
                  style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }}
                />
                {attachment.status === "uploading" ? (
                  <div
                    aria-label="Uploading image"
                    style={{
                      position: "absolute",
                      inset: 0,
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      background: "rgba(24,24,27,0.42)",
                      color: "#fff",
                      fontSize: 11,
                      fontWeight: 700,
                    }}
                  >
                    ...
                  </div>
                ) : null}
                <button
                  type="button"
                  aria-label={`Remove ${attachment.filename}`}
                  onClick={() => onRemoveImageAttachment?.(attachment.id)}
                  style={{
                    position: "absolute",
                    top: 4,
                    right: 4,
                    width: 20,
                    height: 20,
                    borderRadius: 999,
                    border: "none",
                    padding: 0,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    background: "rgba(24,24,27,0.74)",
                    color: "#fff",
                    cursor: "pointer",
                  }}
                >
                  <X size="13" strokeWidth={2.2} />
                </button>
              </div>
            ))}
          </div>
        ) : null}
        {imageUploadEnabled ? (
          <>
            {showAttachmentMenu && !disabled && !isGenerating ? (
              <div
                role="menu"
                aria-label="Attachment menu"
                style={{
                  position: "absolute",
                  left: 0,
                  bottom: "calc(100% + 8px)",
                  zIndex: 30,
                  width: "min(300px, calc(100vw - 48px))",
                  boxSizing: "border-box",
                  padding: "0.875rem 1rem",
                  borderRadius: 20,
                  border: border("var(--copilot-attachment-menu-border, rgba(212,212,216,0.9))"),
                  background: "var(--copilot-attachment-menu-bg, rgba(255,255,255,0.98))",
                  boxShadow: "var(--copilot-attachment-menu-shadow, 0 18px 46px rgba(24,24,27,0.16), 0 2px 8px rgba(24,24,27,0.08))",
                  color: "var(--copilot-attachment-menu-fg, rgba(24,24,27,0.96))",
                }}
              >
                {[
                  {
                    label: imageButtonLabel,
                    icon: <Paperclip size="21" strokeWidth={2.3} />,
                    action: () => {
                      setShowAttachmentMenu(false);
                      fileInputRef.current?.click();
                    },
                  },
                  {
                    label: "Recent files",
                    icon: <FileText size="21" strokeWidth={2.2} />,
                    chevron: true,
                  },
                  { divider: true },
                  {
                    label: "Create image",
                    icon: <ImagePlus size="21" strokeWidth={2.2} />,
                  },
                  {
                    label: "Deep research",
                    icon: <Telescope size="21" strokeWidth={2.2} />,
                  },
                  {
                    label: "Web search",
                    icon: <Globe size="21" strokeWidth={2.2} />,
                  },
                  {
                    label: "More",
                    icon: <Ellipsis size="21" strokeWidth={2.2} />,
                    chevron: true,
                  },
                ].map((item, index) => {
                  if ("divider" in item) {
                    return (
                      <div
                        key={`divider-${index}`}
                        role="separator"
                        style={{
                          height: 1,
                          margin: "0.5rem 0.25rem",
                          background: "var(--copilot-attachment-menu-divider, rgba(212,212,216,0.88))",
                        }}
                      />
                    );
                  }
                  const isActive = Boolean(item.action);
                  const isHovered = hoveredAttachmentIndex === index;
                  return (
                    <button
                      key={item.label}
                      type="button"
                      role="menuitem"
                      aria-disabled={!isActive}
                      onMouseDown={(event) => event.preventDefault()}
                      onMouseEnter={() => setHoveredAttachmentIndex(index)}
                      onMouseLeave={() => setHoveredAttachmentIndex(null)}
                      onClick={() => item.action?.()}
                      style={{
                        width: "100%",
                        minHeight: 44,
                        boxSizing: "border-box",
                        border: "none",
                        borderRadius: 10,
                        padding: "0.375rem 0.25rem",
                        display: "grid",
                        gridTemplateColumns: "32px minmax(0, 1fr) 20px",
                        alignItems: "center",
                        gap: 8,
                        background: isHovered && isActive
                          ? "var(--copilot-attachment-menu-hover-bg, rgba(24,24,27,0.07))"
                          : "transparent",
                        color: "inherit",
                        cursor: isActive ? "pointer" : "default",
                        fontFamily: "inherit",
                        fontSize: 16,
                        lineHeight: 1.2,
                        textAlign: "left",
                      }}
                    >
                      <span
                        aria-hidden="true"
                        style={{
                          width: 32,
                          height: 32,
                          display: "inline-flex",
                          alignItems: "center",
                          justifyContent: "center",
                          color: "var(--copilot-attachment-menu-icon-fg, rgba(24,24,27,0.96))",
                        }}
                      >
                        {item.icon}
                      </span>
                      <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                        {item.label}
                      </span>
                      {item.chevron ? (
                        <ChevronRight
                          aria-hidden="true"
                          size="20"
                          strokeWidth={2.1}
                          style={{ justifySelf: "end" }}
                        />
                      ) : null}
                    </button>
                  );
                })}
              </div>
            ) : null}
            <input
              ref={fileInputRef}
              type="file"
              accept={imageAccept}
              multiple
              tabIndex={-1}
              aria-hidden="true"
              style={{ display: "none" }}
              onChange={(event) => {
                const files = Array.from(event.target.files ?? []);
                event.target.value = "";
                if (files.length > 0) {
                  onImageFilesSelected?.(files);
                }
              }}
            />
            <button
              type="button"
              aria-label="Open attachment menu"
              aria-expanded={showAttachmentMenu}
              aria-haspopup="menu"
              title="Open attachment menu"
              disabled={disabled || isGenerating}
              onClick={() => setShowAttachmentMenu((current) => !current)}
              style={{
                width: buttonSize,
                height: buttonSize,
                boxSizing: "border-box",
                flexShrink: 0,
                borderRadius: 999,
                border: "none",
                padding: 0,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                cursor: disabled || isGenerating ? "not-allowed" : "pointer",
                opacity: disabled || isGenerating ? 0.5 : 1,
                background: "var(--copilot-secondary-control-bg, rgba(24,24,27,0.08))",
                color: "var(--copilot-secondary-control-fg, rgba(39,39,42,0.88))",
              }}
            >
              <Plus size="19" strokeWidth={2.1} />
            </button>
          </>
        ) : null}
        <textarea
          ref={textareaRef}
          className="talon-chat-input-textarea"
          value={value}
          onChange={(event) => onValueChange(event.target.value)}
          placeholder={placeholder}
          autoFocus={autoFocus}
          disabled={disabled}
          rows={rows}
          style={{
            flex: 1,
            boxSizing: "border-box",
            resize: "none",
            border: "none",
            outline: "none",
            boxShadow: "none",
            background: "transparent",
            padding: isSingleLine ? "0 0.4rem" : "0.25rem 0.4rem",
            maxHeight: textareaMaxHeight,
            minHeight: resolvedTextareaMinHeight,
            height: isSingleLine ? buttonSize : undefined,
            fontFamily: "inherit",
            fontSize: 14,
            lineHeight: `${textareaLineHeight}px`,
            overflowY: isSingleLine ? "hidden" : "auto",
            color: "inherit",
            appearance: "none",
            WebkitAppearance: "none",
          }}
          onKeyDown={(event) => {
            if (shouldShowCommandMenu && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
              event.preventDefault();
              setHighlightedCommandIndex((current) => {
                const delta = event.key === "ArrowDown" ? 1 : -1;
                return (current + delta + visibleCommandItems.length) % visibleCommandItems.length;
              });
              return;
            }
            if (shouldShowCommandMenu && event.key === "Tab" && highlightedCommand) {
              event.preventDefault();
              selectCommand(highlightedCommand);
              return;
            }
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault();
              submitValue();
            }
          }}
        />
        <button
          type={isStopMode ? "button" : "submit"}
          onClick={isStopMode && onStop ? onStop : undefined}
          disabled={buttonDisabled}
          aria-label={isStopMode ? stopLabel : submitLabel}
          style={{
            width: buttonSize,
            height: buttonSize,
            boxSizing: "border-box",
            flexShrink: 0,
            borderRadius: 999,
            border: "none",
            padding: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            cursor: buttonIsEnabled ? "pointer" : "not-allowed",
            opacity: buttonIsEnabled ? 1 : 0.5,
            background: "var(--copilot-control-bg, var(--foreground, #18181b))",
            color: "var(--copilot-control-fg, var(--background, #ffffff))",
          }}
        >
          {isStopMode ? (
            <Square size="16" strokeWidth={2} fill="currentColor" />
          ) : (
            <ArrowUp size="16" strokeWidth={2.2} />
          )}
        </button>
      </form>
      {helperText ? (
        <div style={{ textAlign: "center", marginTop: 12, fontSize: 11, opacity: 0.6 }}>
          {helperText}
        </div>
      ) : null}
    </>
  );
}
