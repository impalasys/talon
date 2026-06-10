import React, { useCallback, useMemo, useRef, useState } from "react";
import { ArrowUp, Square, Terminal } from "lucide-react";

function border(color: string) {
  return `1px solid ${color}`;
}

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
  style?: React.CSSProperties;
};

export type ChatInputCommandMenuItem = {
  name: string;
  aliases?: string[];
  description?: string;
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
  style,
}: ChatInputBoxProps) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const [highlightedCommandIndex, setHighlightedCommandIndex] = useState(0);
  const [hoveredCommandIndex, setHoveredCommandIndex] = useState<number | null>(null);
  const resolvedCanSubmit = canSubmit ?? (Boolean(value.trim()) && !disabled && !isGenerating);
  const isStopMode = Boolean(isGenerating && onStop);
  const resolvedCanStop = Boolean(isStopMode && canStop);
  const buttonDisabled = isStopMode ? !resolvedCanStop : !resolvedCanSubmit;
  const buttonIsEnabled = !buttonDisabled;
  const buttonSize = 30;
  const isSingleLine = rows <= 1;
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
          ...style,
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
