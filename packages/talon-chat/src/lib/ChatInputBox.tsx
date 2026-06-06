import React, { useCallback } from "react";
import { ArrowUp, Square } from "lucide-react";

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
  style?: React.CSSProperties;
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
  style,
}: ChatInputBoxProps) {
  const resolvedCanSubmit = canSubmit ?? (Boolean(value.trim()) && !disabled && !isGenerating);
  const isStopMode = Boolean(isGenerating && onStop);
  const resolvedCanStop = Boolean(isStopMode && canStop);
  const buttonDisabled = isStopMode ? !resolvedCanStop : !resolvedCanSubmit;
  const buttonIsEnabled = !buttonDisabled;
  const buttonSize = 30;
  const isSingleLine = rows <= 1;
  const textareaLineHeight = isSingleLine ? buttonSize : 20;
  const resolvedTextareaMinHeight = rows > 1 ? textareaMinHeight : buttonSize;

  const submitValue = useCallback(() => {
    if (!resolvedCanSubmit) return;
    onSubmit(value);
  }, [onSubmit, resolvedCanSubmit, value]);

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
        <textarea
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
            if (event.key === "Enter" && !event.shiftKey) {
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
