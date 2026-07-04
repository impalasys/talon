import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowUp,
  ImagePlus,
  Plus,
  Square,
  Terminal,
  X,
} from "lucide-react";

function border(color: string) {
  return `1px solid ${color}`;
}

const talonChatFontFamily =
  'var(--talon-chat-font-family, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif)';

const composerTextareaFontSize = 16;
const composerTextareaLineHeight = 20;
const composerMutedForeground = "var(--copilot-composer-muted-fg, var(--copilot-input-placeholder, rgba(82,82,91,0.72)))";
const composerVariantTransition =
  "min-height 260ms cubic-bezier(0.22, 1, 0.36, 1), padding 260ms cubic-bezier(0.22, 1, 0.36, 1), border-radius 260ms cubic-bezier(0.22, 1, 0.36, 1), box-shadow 260ms ease, background 220ms ease, border-color 220ms ease, gap 260ms cubic-bezier(0.22, 1, 0.36, 1)";

export type TalonChatComposerVariant = "panel" | "compact" | "expanded" | "inline";

type TalonChatComposerVariantStyle = {
  attachmentRadius: number;
  attachmentSize: number;
  backdropFilter?: string;
  border: string;
  borderRadius: string;
  boxShadow: string;
  buttonSize: number;
  controlsBelow?: boolean;
  gap: number;
  minHeight?: number;
  padding: string;
  textareaMinHeight?: number;
  textareaPadding: string;
  background: string;
};

const composerVariantStyles: Record<TalonChatComposerVariant, TalonChatComposerVariantStyle> = {
  panel: {
    attachmentRadius: 8,
    attachmentSize: 58,
    backdropFilter: "blur(12px)",
    border: border("var(--copilot-input-border, rgba(212,212,216,0.72))"),
    borderRadius: "var(--copilot-input-radius, 22px)",
    boxShadow: "var(--copilot-input-shadow, 0 4px 14px rgba(24,24,27,0.08), 0 1px 2px rgba(24,24,27,0.06))",
    buttonSize: 34,
    gap: 8,
    padding: "0.25rem 0.3125rem 0.25rem 0.625rem",
    textareaPadding: "7px 0.4rem",
    background: "var(--copilot-input-bg, rgba(255,255,255,0.96))",
  },
  compact: {
    attachmentRadius: 7,
    attachmentSize: 48,
    backdropFilter: "blur(10px)",
    border: border("var(--copilot-input-compact-border, rgba(212,212,216,0.72))"),
    borderRadius: "var(--copilot-input-compact-radius, 16px)",
    boxShadow: "var(--copilot-input-compact-shadow, 0 2px 8px rgba(24,24,27,0.07), 0 1px 2px rgba(24,24,27,0.05))",
    buttonSize: 30,
    gap: 6,
    padding: "0.1875rem 0.25rem 0.1875rem 0.5rem",
    textareaPadding: "5px 0.3rem",
    background: "var(--copilot-input-compact-bg, rgba(255,255,255,0.96))",
  },
  expanded: {
    attachmentRadius: 10,
    attachmentSize: 64,
    backdropFilter: "blur(18px)",
    border: border("var(--copilot-input-expanded-border, rgba(212,212,216,0.78))"),
    borderRadius: "var(--copilot-input-expanded-radius, 28px)",
    boxShadow: "var(--copilot-input-expanded-shadow, 0 6px 18px rgba(24,24,27,0.06), 0 1px 2px rgba(24,24,27,0.05))",
    buttonSize: 34,
    controlsBelow: true,
    gap: 8,
    minHeight: 110,
    padding: "0.875rem 0.625rem 0.625rem 0.875rem",
    textareaMinHeight: 42,
    textareaPadding: "0 0.375rem",
    background: "var(--copilot-input-expanded-bg, rgba(255,255,255,0.94))",
  },
  inline: {
    attachmentRadius: 7,
    attachmentSize: 46,
    border: border("var(--copilot-input-inline-border, rgba(212,212,216,0.64))"),
    borderRadius: "var(--copilot-input-inline-radius, 10px)",
    boxShadow: "var(--copilot-input-inline-shadow, none)",
    buttonSize: 30,
    gap: 6,
    padding: "0.125rem 0.25rem 0.125rem 0.375rem",
    textareaPadding: "5px 0.25rem",
    background: "var(--copilot-input-inline-bg, transparent)",
  },
};

export type TalonChatComposerProps = {
  value: string;
  onValueChange: (value: string) => void;
  onSubmit: (value: string) => void;
  placeholder: string;
  variant?: TalonChatComposerVariant;
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
  commandMenuItems?: TalonChatComposerCommandMenuItem[];
  startAdornment?: React.ReactNode;
  endAdornment?: React.ReactNode;
  imageAttachments?: TalonChatComposerImageAttachment[];
  imageUploadEnabled?: boolean;
  imageAccept?: string;
  imageButtonLabel?: string;
  onImageFilesSelected?: (files: File[]) => void;
  onRemoveImageAttachment?: (id: string) => void;
  style?: React.CSSProperties;
};

export type TalonChatComposerCommandMenuItem = {
  name: string;
  aliases?: string[];
  description?: string;
};

export type TalonChatComposerImageAttachment = {
  id: string;
  filename: string;
  previewUrl: string;
  status?: "queued" | "uploading" | "ready" | "error";
  error?: string;
};

export function TalonChatComposer({
  value,
  onValueChange,
  onSubmit,
  placeholder,
  variant = "panel",
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
  startAdornment,
  endAdornment,
  imageAttachments,
  imageUploadEnabled = false,
  imageAccept = "image/png,image/jpeg,image/gif,image/webp",
  imageButtonLabel = "Add image",
  onImageFilesSelected,
  onRemoveImageAttachment,
  style,
}: TalonChatComposerProps) {
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
  const variantStyle = composerVariantStyles[variant];
  const controlsBelow = Boolean(variantStyle.controlsBelow);
  const attachmentMenuWidth = controlsBelow ? "auto" : "min(176px, calc(100vw - 48px))";
  const buttonSize = variantStyle.buttonSize;
  const attachments = imageAttachments ?? [];
  const textareaLineHeight = composerTextareaLineHeight;
  const resolvedTextareaMinHeight = Math.max(
    rows > 1 ? textareaMinHeight : buttonSize,
    variantStyle.textareaMinHeight ?? 0,
  );
  const [textareaSize, setTextareaSize] = useState<{ height: number; overflowY: "auto" | "hidden" }>({
    height: resolvedTextareaMinHeight,
    overflowY: "hidden",
  });
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

  const selectCommand = useCallback((item: TalonChatComposerCommandMenuItem) => {
    onValueChange(`/${item.name}`);
    window.requestAnimationFrame(() => {
      textareaRef.current?.focus();
    });
  }, [onValueChange]);

  const measureTextarea = useCallback(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;

    const previousHeight = textarea.style.height;
    textarea.style.height = "auto";
    const computedMaxHeight = window.getComputedStyle(textarea).maxHeight;
    const numericMaxHeight = computedMaxHeight && computedMaxHeight !== "none"
      ? Number.parseFloat(computedMaxHeight)
      : Number.NaN;
    const maxHeight = Number.isFinite(numericMaxHeight) ? numericMaxHeight : Number.POSITIVE_INFINITY;
    const scrollHeight = textarea.scrollHeight;
    const nextHeight = Math.ceil(Math.max(resolvedTextareaMinHeight, Math.min(scrollHeight, maxHeight)));
    const nextOverflowY = scrollHeight > maxHeight + 1 ? "auto" : "hidden";
    textarea.style.height = previousHeight;

    setTextareaSize((current) => (
      current.height === nextHeight && current.overflowY === nextOverflowY
        ? current
        : { height: nextHeight, overflowY: nextOverflowY }
    ));
  }, [resolvedTextareaMinHeight]);

  useEffect(() => {
    measureTextarea();
  }, [attachments.length, measureTextarea, rows, value]);

  useEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;

    let frameId = 0;
    const scheduleMeasure = () => {
      window.cancelAnimationFrame(frameId);
      frameId = window.requestAnimationFrame(measureTextarea);
    };

    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", scheduleMeasure);
      return () => {
        window.cancelAnimationFrame(frameId);
        window.removeEventListener("resize", scheduleMeasure);
      };
    }

    const observer = new ResizeObserver(scheduleMeasure);
    observer.observe(textarea);
    return () => {
      window.cancelAnimationFrame(frameId);
      observer.disconnect();
    };
  }, [measureTextarea]);

  return (
    <>
      <style>
        {`
          .talon-chat-input-textarea::placeholder {
            color: ${composerMutedForeground};
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
          gap: variantStyle.gap,
          rowGap: controlsBelow ? 8 : variantStyle.gap,
          width: "100%",
          minHeight: variantStyle.minHeight,
          boxSizing: "border-box",
          borderRadius: variantStyle.borderRadius,
          border: variantStyle.border,
          background: variantStyle.background,
          boxShadow: variantStyle.boxShadow,
          padding: variantStyle.padding,
          backdropFilter: variantStyle.backdropFilter,
          transition: composerVariantTransition,
          flexWrap: controlsBelow || attachments.length > 0 ? "wrap" : "nowrap",
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
              order: controlsBelow ? 0 : undefined,
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
                  width: variantStyle.attachmentSize,
                  height: variantStyle.attachmentSize,
                  flexShrink: 0,
                  overflow: "hidden",
                  borderRadius: variantStyle.attachmentRadius,
                  border: border(
                    attachment.status === "error"
                      ? "var(--copilot-attachment-error-border, rgba(220,38,38,0.55))"
                      : "var(--copilot-attachment-border, rgba(212,212,216,0.9))",
                  ),
                  background: "var(--copilot-attachment-bg, rgba(244,244,245,0.88))",
                  transition: "width 220ms ease, height 220ms ease, border-radius 220ms ease",
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
                  right: controlsBelow ? 0 : undefined,
                  bottom: controlsBelow ? "calc(100% + 16px)" : "calc(100% + 6px)",
                  zIndex: 30,
                  width: attachmentMenuWidth,
                  maxHeight: controlsBelow ? "min(360px, calc(100vh - 220px))" : undefined,
                  boxSizing: "border-box",
                  overflowY: controlsBelow ? "auto" : undefined,
                  padding: controlsBelow ? "0.625rem" : "0.375rem",
                  borderRadius: controlsBelow ? variantStyle.borderRadius : 16,
                  border: border("var(--copilot-attachment-menu-border, rgba(212,212,216,0.9))"),
                  background: "var(--copilot-attachment-menu-bg, rgba(255,255,255,0.98))",
                  boxShadow: controlsBelow
                    ? "var(--copilot-attachment-menu-expanded-shadow, var(--copilot-input-expanded-shadow, 0 6px 18px rgba(24,24,27,0.06), 0 1px 2px rgba(24,24,27,0.05)))"
                    : "var(--copilot-attachment-menu-shadow, 0 14px 30px rgba(24,24,27,0.13), 0 2px 8px rgba(24,24,27,0.07))",
                  color: composerMutedForeground,
                }}
              >
                {controlsBelow ? (
                  <div
                    style={{
                      padding: "0 0.5rem 0.375rem",
                      color: composerMutedForeground,
                      fontSize: 14,
                      lineHeight: 1.3,
                    }}
                  >
                    Add
                  </div>
                ) : null}
                <button
                  type="button"
                  role="menuitem"
                  onMouseDown={(event) => event.preventDefault()}
                  onMouseEnter={() => setHoveredAttachmentIndex(0)}
                  onMouseLeave={() => setHoveredAttachmentIndex(null)}
                  onClick={() => {
                    setShowAttachmentMenu(false);
                    fileInputRef.current?.click();
                  }}
                  style={{
                    width: "100%",
                    minHeight: controlsBelow ? 44 : 34,
                    boxSizing: "border-box",
                    border: "none",
                    borderRadius: controlsBelow ? 12 : 10,
                    padding: controlsBelow ? "0.375rem 0.5rem" : "0.25rem 0.375rem",
                    display: "grid",
                    gridTemplateColumns: controlsBelow ? "36px minmax(0, 1fr)" : "26px minmax(0, 1fr)",
                    alignItems: "center",
                    gap: 8,
                    background: hoveredAttachmentIndex === 0
                      ? "var(--copilot-attachment-menu-hover-bg, rgba(24,24,27,0.07))"
                      : "transparent",
                    color: composerMutedForeground,
                    cursor: "pointer",
                    fontFamily: "inherit",
                    fontSize: controlsBelow ? 15 : 14,
                    lineHeight: 1.2,
                    textAlign: "left",
                  }}
                >
                  <span
                    aria-hidden="true"
                    style={{
                      width: controlsBelow ? 36 : 26,
                      height: controlsBelow ? 36 : 26,
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      color: composerMutedForeground,
                    }}
                  >
                    <ImagePlus size={controlsBelow ? "19" : "17"} strokeWidth="2.1" />
                  </span>
                  <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {imageButtonLabel}
                  </span>
                </button>
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
                order: controlsBelow ? 2 : undefined,
                flexShrink: 0,
                borderRadius: 999,
                border: "none",
                padding: 0,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                cursor: disabled || isGenerating ? "not-allowed" : "pointer",
                opacity: disabled || isGenerating ? 0.5 : 1,
                background: "transparent",
                color: composerMutedForeground,
                transition: "width 220ms ease, height 220ms ease, opacity 160ms ease, color 160ms ease",
              }}
            >
              <Plus size="19" strokeWidth={2.1} />
            </button>
          </>
        ) : null}
        {startAdornment ? (
          <div
            style={{
              order: controlsBelow ? 2 : undefined,
              flexShrink: 0,
              display: "flex",
              alignItems: "center",
            }}
          >
            {startAdornment}
          </div>
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
            order: controlsBelow ? 1 : undefined,
            flex: controlsBelow ? "0 0 100%" : 1,
            boxSizing: "border-box",
            resize: "none",
            border: "none",
            outline: "none",
            boxShadow: "none",
            background: "transparent",
            padding: variantStyle.textareaPadding,
            maxHeight: textareaMaxHeight,
            minHeight: resolvedTextareaMinHeight,
            height: textareaSize.height,
            fontFamily: "inherit",
            fontSize: composerTextareaFontSize,
            lineHeight: `${textareaLineHeight}px`,
            overflowY: textareaSize.overflowY,
            color: "inherit",
            appearance: "none",
            WebkitAppearance: "none",
            transition: "height 220ms cubic-bezier(0.22, 1, 0.36, 1), min-height 220ms cubic-bezier(0.22, 1, 0.36, 1), padding 220ms ease",
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
        {endAdornment ? (
          <div
            style={{
              order: controlsBelow ? 2 : undefined,
              flexShrink: 0,
              display: "flex",
              alignItems: "center",
            }}
          >
            {endAdornment}
          </div>
        ) : null}
        <button
          type={isStopMode ? "button" : "submit"}
          onClick={isStopMode && onStop ? onStop : undefined}
          disabled={buttonDisabled}
          aria-label={isStopMode ? stopLabel : submitLabel}
          style={{
            width: buttonSize,
            height: buttonSize,
            boxSizing: "border-box",
            order: controlsBelow ? 3 : undefined,
            flexShrink: 0,
            marginLeft: controlsBelow ? "auto" : undefined,
            borderRadius: 999,
            border: "none",
            padding: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            cursor: buttonIsEnabled ? "pointer" : "not-allowed",
            opacity: buttonIsEnabled ? 1 : 0.5,
            background: "var(--copilot-control-bg, var(--foreground, #8e8e93))",
            color: "var(--copilot-control-fg, var(--background, #ffffff))",
            transition: "width 220ms ease, height 220ms ease, margin-left 220ms ease, opacity 160ms ease, background 160ms ease",
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
