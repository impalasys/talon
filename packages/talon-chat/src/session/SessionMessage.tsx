"use client";

import React from "react";
import { ChevronRight } from "lucide-react";
import {
  formatUsageSummary,
  getMessageAssistantTimeline,
  getMessageContent,
  getMessageReasoningContent,
  getMessageUsage,
  type CopilotMessage,
} from "../lib/chatTimeline";
import { MarkdownMessage } from "../lib/MarkdownMessage";
import { objectRefFromPart } from "./objectRefs";
import { parsePayloadJson, SESSION_MESSAGE_PART_TYPE } from "./protocol";
import {
  AssistantMessageTimeline,
  coalesceAssistantMessageTimelineForDisplay,
  splitAssistantMessageTimeline,
} from "./AssistantMessageTimeline";
import { AssistantMessage } from "./AssistantMessage";
import { ConnectorDeliveryControls } from "./ConnectorDeliveryControls";
import { MessageActions } from "./MessageActions";
import { MessageEditForm } from "./MessageEditForm";
import { MessageImages, type MessageImage } from "./MessageImages";
import { formatWorkDuration, formatWorkingDuration } from "./sessionTiming";
import type { TalonChatObjectRef } from "./types";
import type { ToolResultHydrationState } from "./hooks/useToolResultHydration";
import { historyMessageTimestamp } from "./history";

const connectorDeliveryPendingReview = "pending_review";
const connectorDeliveryStatusLabel = "talon.impalasys.com/connector-delivery-status";
const messageFontSize = "var(--talon-chat-message-font-size, 1rem)";

function border(color: string) {
  return `1px solid ${color}`;
}

function classNames(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(" ");
}

function isImagePart(part: any) {
  const type = part?.type ?? part?.partType ?? part?.part_type;
  return type === "image" || type === SESSION_MESSAGE_PART_TYPE.IMAGE || type === "SESSION_MESSAGE_PART_TYPE_IMAGE";
}

function imageSource(
  part: any,
  payload: Record<string, unknown>,
  object: TalonChatObjectRef | undefined,
  objectUrlForRef?: (object: TalonChatObjectRef) => string | undefined,
) {
  if (typeof part.previewUrl === "string") return part.previewUrl;
  if (typeof part.url === "string") return part.url;
  if (typeof payload.url === "string") return payload.url;
  return object ? objectUrlForRef?.(object) : undefined;
}

function imageLabel(object: TalonChatObjectRef | undefined, payload: Record<string, unknown>, index: number) {
  if (object?.filename) return object.filename;
  if (typeof payload.filename === "string") return payload.filename;
  return object?.key || `image-${index + 1}`;
}

function imageFromPart(
  message: CopilotMessage,
  part: any,
  index: number,
  objectUrlForRef?: (object: TalonChatObjectRef) => string | undefined,
): MessageImage | null {
  if (!isImagePart(part)) return null;
  const payload = parsePayloadJson(part.payloadJson ?? part.payload_json);
  const object = objectRefFromPart(part);
  return {
    id: `${message.id}-image-${index}`,
    src: imageSource(part, payload, object, objectUrlForRef),
    label: imageLabel(object, payload, index),
  };
}

function messageImages(
  message: CopilotMessage,
  objectUrlForRef?: (object: TalonChatObjectRef) => string | undefined,
): MessageImage[] {
  if (!Array.isArray(message.parts)) return [];
  return message.parts.flatMap((part: any, index) => {
    const image = imageFromPart(message, part, index, objectUrlForRef);
    return image ? [image] : [];
  });
}

function actionTimestamp(message: CopilotMessage) {
  const timestampMs = historyMessageTimestamp(message);
  if (timestampMs === null) return null;
  return new Date(timestampMs).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

export type SessionMessageProps = {
  message: CopilotMessage;
  messageIndex: number;
  messages: CopilotMessage[];
  isSessionLive: boolean;
  loadingStartedAt: string | number | null;
  loadingNow: number;
  showWorkDetails: boolean;
  objectUrlForRef?: (object: TalonChatObjectRef) => string | undefined;
  allowEditing: boolean;
  enableDebugEditing: boolean;
  editingMessageId: string | null;
  editingMessageValue: string;
  reviewActionMessageId: string | null;
  expandedThinkingMessages: Record<string, boolean>;
  expandedToolItems: Record<string, boolean>;
  hydrationState: Record<string, ToolResultHydrationState>;
  resultFor: (message: CopilotMessage, toolCallId: string, fallback: unknown) => unknown;
  onToggleThinking: (messageId: string) => void;
  onToggleTool: (key: string) => void;
  onHydrateTool: (message: CopilotMessage, toolCallId: string, key: string, fallback: unknown) => void;
  onResourceClick: (uri: string) => void;
  onEditingValueChange: (value: string) => void;
  onSaveEdit: (message: CopilotMessage) => void;
  onCancelEdit: () => void;
  onStartEdit: (message: CopilotMessage) => void;
  onCopy: (message: CopilotMessage) => void;
  onUpdateConnectorDelivery: (message: CopilotMessage, status: string) => void;
};

type WorkDetailsProps = Pick<
  SessionMessageProps,
  | "message"
  | "messageIndex"
  | "messages"
  | "isSessionLive"
  | "loadingStartedAt"
  | "loadingNow"
  | "expandedThinkingMessages"
  | "expandedToolItems"
  | "hydrationState"
  | "resultFor"
  | "onToggleThinking"
  | "onToggleTool"
  | "onHydrateTool"
  | "onResourceClick"
>;

function previousUserMessage(messages: CopilotMessage[], beforeIndex: number) {
  for (let index = beforeIndex - 1; index >= 0; index -= 1) {
    if (messages[index]?.role === "user") return messages[index];
  }
  return undefined;
}

function hasExpandableWork(
  workItemCount: number,
  workHasReasoning: boolean,
  reasoningContent: string,
  workHasUsage: boolean,
  usageSummary: string,
) {
  if (workItemCount > 0) return true;
  if (!workHasReasoning && Boolean(reasoningContent)) return true;
  return !workHasUsage && Boolean(usageSummary);
}

function WorkDetailsHeader({
  canExpand,
  isExpanded,
  label,
  onToggle,
}: {
  canExpand: boolean;
  isExpanded: boolean;
  label: string;
  onToggle: () => void;
}) {
  return (
    <>
      <button
        type="button"
        onClick={onToggle}
        disabled={!canExpand}
        style={{ width: "100%", display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, border: "none", background: "transparent", padding: "0 0 0.65rem", cursor: canExpand ? "pointer" : "default", textAlign: "left", color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}
      >
        <span style={{ fontSize: 13, fontWeight: 500 }}>{label}</span>
        {canExpand ? <ChevronRight size="16" style={{ flexShrink: 0, transform: isExpanded ? "rotate(90deg)" : "rotate(0deg)", transition: "transform 160ms ease", color: "var(--talon-chat-subtle-fg, rgba(113,113,122,0.9))" }} /> : null}
      </button>
      <div style={{ borderTop: border("var(--talon-chat-divider, rgba(212,212,216,0.7))") }} />
    </>
  );
}

type WorkDetailsContentProps = Pick<
  WorkDetailsProps,
  "message" | "expandedToolItems" | "hydrationState" | "resultFor" | "onToggleTool" | "onHydrateTool" | "onResourceClick"
> & {
  items: ReturnType<typeof splitAssistantMessageTimeline>["workTimeline"];
  isLive: boolean;
  showReasoningFallback: boolean;
  reasoningContent: string;
  showUsageFallback: boolean;
  usageSummary: string;
};

function WorkDetailsContent({
  message,
  items,
  isLive,
  expandedToolItems,
  hydrationState,
  resultFor,
  onToggleTool,
  onHydrateTool,
  onResourceClick,
  showReasoningFallback,
  reasoningContent,
  showUsageFallback,
  usageSummary,
}: WorkDetailsContentProps) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, paddingTop: 12 }}>
      <AssistantMessageTimeline message={message} items={items} isLive={isLive} expandedTools={expandedToolItems} hydrationState={hydrationState} resultFor={resultFor} onToggleTool={onToggleTool} onHydrateTool={onHydrateTool} onResourceClick={onResourceClick} />
      {showReasoningFallback ? <div style={{ whiteSpace: "normal", overflowWrap: "break-word", fontSize: 13, lineHeight: 1.55, color: "var(--talon-chat-subtle-fg, rgba(82,82,91,0.96))" }}>{reasoningContent}</div> : null}
      {showUsageFallback ? <div style={{ fontSize: 12, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>{usageSummary}</div> : null}
    </div>
  );
}

function MessageWorkDetails({
  message,
  messageIndex,
  messages,
  isSessionLive,
  loadingStartedAt,
  loadingNow,
  expandedThinkingMessages,
  expandedToolItems,
  hydrationState,
  resultFor,
  onToggleThinking,
  onToggleTool,
  onHydrateTool,
  onResourceClick,
}: WorkDetailsProps) {
  if (message.role !== "assistant") return null;
  const isLive = isSessionLive && messageIndex === messages.length - 1;
  const timeline = coalesceAssistantMessageTimelineForDisplay(getMessageAssistantTimeline(message));
  const workTimeline = splitAssistantMessageTimeline(timeline).workTimeline;
  const reasoningContent = getMessageReasoningContent(message);
  const usageSummary = formatUsageSummary(getMessageUsage(message));
  const workHasReasoning = workTimeline.some((item) => item.type === "reasoning");
  const workHasUsage = workTimeline.some((item) => item.type === "usage");
  const canExpand = hasExpandableWork(
    workTimeline.length,
    workHasReasoning,
    reasoningContent,
    workHasUsage,
    usageSummary,
  );
  if (!canExpand && !isLive) return null;
  const isExpanded = isLive || (expandedThinkingMessages[message.id] ?? false);
  const label = isLive
    ? formatWorkingDuration(loadingStartedAt, loadingNow)
    : formatWorkDuration(previousUserMessage(messages, messageIndex)?.createdAt, message.createdAt);
  const toggle = () => { if (canExpand) onToggleThinking(message.id); };

  return (
    <div style={{ marginBottom: 16 }}>
      <WorkDetailsHeader canExpand={canExpand} isExpanded={isExpanded} label={label} onToggle={toggle} />
      {isExpanded ? <WorkDetailsContent {...{
        message, items: workTimeline, isLive, expandedToolItems, hydrationState,
        resultFor, onToggleTool, onHydrateTool, onResourceClick,
        showReasoningFallback: !workHasReasoning && Boolean(reasoningContent), reasoningContent,
        showUsageFallback: !workHasUsage && Boolean(usageSummary), usageSummary,
      }} /> : null}
    </div>
  );
}

type MessageContentProps = Pick<
  SessionMessageProps,
  "message" | "isSessionLive" | "expandedToolItems" | "hydrationState" | "resultFor" | "onToggleTool" | "onHydrateTool" | "onResourceClick"
> & {
  isLiveAssistantMessage: boolean;
  content: string;
  renderNode?: React.ReactNode;
};

function MessageContent({
  message,
  isLiveAssistantMessage,
  content,
  renderNode,
  expandedToolItems,
  hydrationState,
  resultFor,
  onToggleTool,
  onHydrateTool,
  onResourceClick,
}: MessageContentProps) {
  if (renderNode !== undefined) {
    return (
      <div
        className={classNames(message.role === "system" && "copilot-system-message")}
        style={{ minWidth: 0, overflow: "hidden", overflowWrap: "anywhere", whiteSpace: "normal", fontSize: message.role === "system" ? 12 : messageFontSize, lineHeight: 1.65, opacity: message.role === "system" ? 0.72 : 0.94, fontFamily: message.role === "system" ? "ui-monospace, SFMono-Regular, monospace" : undefined }}
      >
        {renderNode}
      </div>
    );
  }
  const timeline = splitAssistantMessageTimeline(
    coalesceAssistantMessageTimelineForDisplay(getMessageAssistantTimeline(message)),
  ).finalTimeline;
  return (
    <div
      className={classNames(message.role === "system" && "copilot-system-message")}
      style={{ minWidth: 0, overflow: "hidden", overflowWrap: "anywhere", whiteSpace: message.role === "assistant" ? "normal" : "pre-wrap", fontSize: message.role === "system" ? 12 : messageFontSize, lineHeight: 1.65, opacity: message.role === "system" ? 0.72 : 0.94, fontFamily: message.role === "system" ? "ui-monospace, SFMono-Regular, monospace" : undefined }}
    >
      {message.role === "assistant" && timeline.length > 0 ? (
        <AssistantMessage message={message} items={timeline} content={content} onResourceClick={onResourceClick} />
      ) : message.role === "assistant" ? <MarkdownMessage onResourceClick={onResourceClick}>{content}</MarkdownMessage> : content}
    </div>
  );
}

type MessageDisplayState = {
  isUser: boolean;
  isLiveAssistant: boolean;
  isEditable: boolean;
  isEditing: boolean;
  isPendingConnectorDelivery: boolean;
  isReviewActionPending: boolean;
};

function displayState({
  message,
  messageIndex,
  messages,
  isSessionLive,
  allowEditing,
  enableDebugEditing,
  editingMessageId,
  reviewActionMessageId,
}: Pick<SessionMessageProps, "message" | "messageIndex" | "messages" | "isSessionLive" | "allowEditing" | "enableDebugEditing" | "editingMessageId" | "reviewActionMessageId">): MessageDisplayState {
  const isUser = message.role === "user";
  const isLiveAssistant = isSessionLive && messageIndex === messages.length - 1 && message.role === "assistant";
  const hasEditableRole = isUser || message.role === "assistant";
  return {
    isUser,
    isLiveAssistant,
    isEditable: (allowEditing || enableDebugEditing) && hasEditableRole && !isLiveAssistant,
    isEditing: editingMessageId === message.id,
    isPendingConnectorDelivery:
      enableDebugEditing && message.labels?.[connectorDeliveryStatusLabel] === connectorDeliveryPendingReview,
    isReviewActionPending: reviewActionMessageId === message.id,
  };
}

function messageRowStyle(isUser: boolean) {
  return { display: "flex", justifyContent: isUser ? "flex-end" : "stretch", width: "100%" } as const;
}

function messageWidthStyle(isUser: boolean) {
  return { width: isUser ? "auto" : "100%", maxWidth: isUser ? "min(80%, 36rem)" : "100%", overflow: "hidden" } as const;
}

function messageBubbleStyle(isUser: boolean) {
  return {
    overflow: "hidden",
    borderRadius: isUser ? 18 : 0,
    background: isUser ? "var(--talon-chat-user-bubble-bg, rgba(24,24,27,0.07))" : "transparent",
    color: isUser ? "var(--talon-chat-user-bubble-fg, inherit)" : "inherit",
    padding: isUser ? "0.75rem 1rem" : 0,
  } as const;
}

function PendingConnectorDelivery({ message, pending, disabled, onUpdate }: {
  message: CopilotMessage;
  pending: boolean;
  disabled: boolean;
  onUpdate: (message: CopilotMessage, status: string) => void;
}) {
  if (!pending) return null;
  return <ConnectorDeliveryControls message={message} disabled={disabled} onUpdate={onUpdate} />;
}

function MessageEditor({
  message,
  isEditing,
  value,
  onChange,
  onSave,
  onCancel,
  contentProps,
}: {
  message: CopilotMessage;
  isEditing: boolean;
  value: string;
  onChange: (value: string) => void;
  onSave: (message: CopilotMessage) => void;
  onCancel: () => void;
  contentProps: Omit<MessageContentProps, "message">;
}) {
  if (!isEditing) return <MessageContent message={message} {...contentProps} />;
  return <MessageEditForm message={message} value={value} onChange={onChange} onSave={onSave} onCancel={onCancel} />;
}

function MessageActionRow({
  message,
  editable,
  editing,
  onCopy,
  onEdit,
}: {
  message: CopilotMessage;
  editable: boolean;
  editing: boolean;
  onCopy: (message: CopilotMessage) => void;
  onEdit: (message: CopilotMessage) => void;
}) {
  if (!editable || editing) return null;
  return <MessageActions message={message} timestamp={actionTimestamp(message)} onCopy={onCopy} onEdit={onEdit} />;
}

function SessionMessagePresentation(props: SessionMessageProps) {
  const {
    message, messageIndex, messages, isSessionLive, loadingStartedAt, loadingNow, objectUrlForRef,
    showWorkDetails,
    editingMessageValue, expandedThinkingMessages, expandedToolItems, hydrationState, resultFor,
    onToggleThinking, onToggleTool, onHydrateTool, onResourceClick, onEditingValueChange,
    onSaveEdit, onCancelEdit, onStartEdit, onCopy, onUpdateConnectorDelivery,
  } = props;
  const content = getMessageContent(message);
  const state = displayState(props);
  const contentProps = {
    isSessionLive,
    isLiveAssistantMessage: state.isLiveAssistant,
    content,
    renderNode: message.renderNode,
    expandedToolItems,
    hydrationState,
    resultFor,
    onToggleTool,
    onHydrateTool,
    onResourceClick,
  };

  return (
    <div data-session-message-id={message.id} className="talon-session-message-row" style={messageRowStyle(state.isUser)}>
      <div style={messageWidthStyle(state.isUser)}>
        <div style={messageBubbleStyle(state.isUser)}>
          {showWorkDetails ? (
            <MessageWorkDetails {...{
              message, messageIndex, messages, isSessionLive, loadingStartedAt, loadingNow,
              expandedThinkingMessages, expandedToolItems, hydrationState, resultFor, onToggleThinking,
              onToggleTool, onHydrateTool, onResourceClick,
            }} />
          ) : null}
          <PendingConnectorDelivery message={message} pending={state.isPendingConnectorDelivery} disabled={state.isReviewActionPending || state.isEditing} onUpdate={onUpdateConnectorDelivery} />
          <MessageEditor message={message} isEditing={state.isEditing} value={editingMessageValue} onChange={onEditingValueChange} onSave={onSaveEdit} onCancel={onCancelEdit} contentProps={contentProps} />
          <MessageImages images={messageImages(message, objectUrlForRef)} hasContent={Boolean(content)} />
        </div>
        <MessageActionRow message={message} editable={state.isEditable} editing={state.isEditing} onCopy={onCopy} onEdit={onStartEdit} />
      </div>
    </div>
  );
}

/** Presentation and interaction boundary for one transcript message. */
export function SessionMessage(props: SessionMessageProps) {
  return <SessionMessagePresentation {...props} />;
}
