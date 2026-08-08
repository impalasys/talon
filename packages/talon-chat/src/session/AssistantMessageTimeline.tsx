"use client";

import React from "react";
import { ChevronRight, Wrench } from "lucide-react";
import {
  formatUsageSummary,
  type AssistantTimelineItem,
  type CopilotMessage,
} from "../lib/chatTimeline";
import { MarkdownMessage } from "../lib/MarkdownMessage";
import type { ToolResultHydrationState } from "./useToolResultHydration";

export type AssistantMessageTimelineProps = {
  message: CopilotMessage;
  items: AssistantTimelineItem[];
  isLive: boolean;
  expandedTools: Record<string, boolean>;
  hydrationState: Record<string, ToolResultHydrationState>;
  resultFor: (message: CopilotMessage, toolCallId: string, fallback: unknown) => unknown;
  onToggleTool: (key: string) => void;
  onHydrateTool: (message: CopilotMessage, toolCallId: string, key: string, fallback: unknown) => void;
  onResourceClick?: (uri: string) => void;
};

function border(color: string) {
  return `1px solid ${color}`;
}

export function coalesceAssistantMessageTimelineForDisplay(timeline: AssistantTimelineItem[]) {
  const nextTimeline: AssistantTimelineItem[] = [];
  let latestUsage: Extract<AssistantTimelineItem, { type: "usage" }> | null = null;

  for (const item of timeline) {
    if (item.type === "usage") {
      latestUsage = item;
      continue;
    }
    if (item.type === "text" || item.type === "reasoning") {
      const lastItem = nextTimeline.at(-1);
      if (lastItem?.type === item.type) {
        nextTimeline[nextTimeline.length - 1] = { type: item.type, text: `${lastItem.text}${item.text}` };
      } else {
        nextTimeline.push(item);
      }
      continue;
    }
    nextTimeline.push(item);
  }

  if (latestUsage) nextTimeline.push(latestUsage);
  return nextTimeline;
}

export function splitAssistantMessageTimeline(timeline: AssistantTimelineItem[]) {
  const displayTimeline = coalesceAssistantMessageTimelineForDisplay(timeline);
  const finalTextIndex = displayTimeline.findLastIndex(
    (item) => item.type === "text" && item.text.trim().length > 0,
  );
  if (finalTextIndex < 0) return { workTimeline: displayTimeline, finalTimeline: [] };
  return {
    workTimeline: displayTimeline.filter((_, index) => index !== finalTextIndex),
    finalTimeline: [displayTimeline[finalTextIndex]],
  };
}

function ContextCompactionDivider({ compacting }: { compacting: boolean }) {
  const label = compacting ? "Context compacting automatically" : "Context compacted";
  return (
    <div
      className="talon-session-compaction-divider"
      role="separator"
      aria-label={label}
      style={{
        display: "flex", alignItems: "center", gap: 10, width: "100%",
        color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))", fontSize: 12,
        fontWeight: 500, letterSpacing: "0.01em", padding: "0.5rem 0",
      }}
    >
      <span aria-hidden="true" style={{ flex: 1, borderTop: border("var(--talon-chat-divider, rgba(212,212,216,0.7))") }} />
      <span>{label}</span>
      <span aria-hidden="true" style={{ flex: 1, borderTop: border("var(--talon-chat-divider, rgba(212,212,216,0.7))") }} />
    </div>
  );
}

type ToolInvocationCardProps = Pick<AssistantMessageTimelineProps,
  "message" | "isLive" | "expandedTools" | "hydrationState" | "resultFor" | "onToggleTool" | "onHydrateTool"
> & {
  item: Extract<AssistantTimelineItem, { type: "tool" }>;
  index: number;
};

function ToolResultDetails({
  args,
  hydration,
  result,
}: {
  args: unknown;
  hydration: ToolResultHydrationState | undefined;
  result: unknown;
}) {
  const output = hydration === "loading"
    ? "Loading output..."
    : hydration
      ? <>
        Historical output is unavailable.
        <details style={{ marginTop: 4 }}><summary>Developer details</summary><code style={{ overflowWrap: "anywhere" }}>{hydration.objectKey}</code></details>
      </>
      : undefined;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10, paddingBottom: 12, paddingLeft: 22 }}>
      <div>
        <div style={{ marginBottom: 6, fontSize: 11, fontWeight: 700, textTransform: "uppercase", color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>Input</div>
        <pre style={{ maxWidth: "100%", overflowX: "auto", whiteSpace: "pre-wrap", overflowWrap: "anywhere", borderRadius: 8, background: "var(--talon-chat-code-bg, rgba(24,24,27,0.05))", padding: 10, fontSize: 12, margin: 0 }}><code>{JSON.stringify(args ?? {}, null, 2)}</code></pre>
      </div>
      {output ? <div style={{ fontSize: 12, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>{output}</div> : null}
      {!hydration && result !== undefined ? (
        <div>
          <div style={{ marginBottom: 6, fontSize: 11, fontWeight: 700, textTransform: "uppercase", color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>Output</div>
          <pre style={{ maxWidth: "100%", overflowX: "auto", whiteSpace: "pre-wrap", overflowWrap: "anywhere", borderRadius: 8, background: "var(--talon-chat-code-bg, rgba(24,24,27,0.05))", padding: 10, fontSize: 12, margin: 0 }}><code>{typeof result === "string" ? result : JSON.stringify(result, null, 2)}</code></pre>
        </div>
      ) : null}
    </div>
  );
}

function ToolInvocationCard({
  message, item, index, isLive, expandedTools, hydrationState, resultFor, onToggleTool, onHydrateTool,
}: ToolInvocationCardProps) {
  const toolKey = `${message.id}-tool-${item.toolCallId || index}`;
  const toolResult = resultFor(message, item.toolCallId, item.result);
  const isExpanded = expandedTools[toolKey] ?? false;
  const isRunning = isLive && toolResult === undefined;
  const hydration = hydrationState[toolKey];
  return (
    <div>
      <button
        className="talon-session-tool-row"
        type="button"
        onClick={() => {
          onToggleTool(toolKey);
          if (!isExpanded) onHydrateTool(message, item.toolCallId, toolKey, toolResult);
        }}
        style={{
          width: "auto", maxWidth: "100%", display: "flex", alignItems: "center", gap: 8,
          border: "none", background: "transparent", padding: "0.25rem 0",
          color: "var(--talon-chat-subtle-fg, rgba(82,82,91,0.96))", cursor: "pointer", textAlign: "left",
        }}
      >
        <Wrench size="14" strokeWidth={1.9} style={{ flexShrink: 0, color: "var(--talon-chat-subtle-fg, rgba(113,113,122,0.9))" }} />
        <span style={{ minWidth: 0, fontSize: 13, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          Called <span style={{ fontFamily: "ui-monospace, SFMono-Regular, monospace" }}>{item.toolName}</span>
        </span>
        {isRunning ? <span style={{ flexShrink: 0, borderRadius: 999, background: "var(--talon-chat-tool-running-bg, rgba(14,165,233,0.12))", color: "var(--talon-chat-tool-running-fg, #0369a1)", padding: "0.1rem 0.45rem", fontSize: 11, fontWeight: 700 }}>Running</span> : null}
        <ChevronRight className="talon-session-tool-chevron" size="14" style={{ flexShrink: 0, transform: isExpanded ? "rotate(90deg)" : "rotate(0deg)", color: "var(--talon-chat-subtle-fg, rgba(113,113,122,0.9))" }} />
      </button>
      {isExpanded ? <ToolResultDetails args={item.args} hydration={hydration} result={toolResult} /> : null}
    </div>
  );
}

function TimelineItem({
  item,
  index,
  props,
}: {
  item: AssistantTimelineItem;
  index: number;
  props: AssistantMessageTimelineProps;
}) {
  const { message, isLive, expandedTools, hydrationState, resultFor, onToggleTool, onHydrateTool, onResourceClick } = props;
  const key = `${message.id}-timeline-${index}`;
  if (item.type === "compaction") return <ContextCompactionDivider compacting={isLive} />;
  if (item.type === "text") return <div style={{ whiteSpace: "normal", overflowWrap: "break-word", fontSize: 13, lineHeight: 1.55, color: "var(--talon-chat-assistant-fg, inherit)" }}><MarkdownMessage onResourceClick={onResourceClick}>{item.text}</MarkdownMessage></div>;
  if (item.type === "reasoning") return <div style={{ whiteSpace: "normal", overflowWrap: "break-word", fontSize: 13, lineHeight: 1.55, color: "var(--talon-chat-subtle-fg, rgba(82,82,91,0.96))" }}>{item.text}</div>;
  if (item.type === "usage") {
    const summary = formatUsageSummary(item.usage);
    return summary ? <div style={{ fontSize: 12, color: "var(--talon-chat-muted-fg, rgba(82,82,91,0.88))" }}>{summary}</div> : null;
  }
  return <ToolInvocationCard key={`${key}-tool-${item.toolCallId}`} {...{ message, item, index, isLive, expandedTools, hydrationState, resultFor, onToggleTool, onHydrateTool }} />;
}

export function AssistantMessageTimeline({
  message, items, isLive, expandedTools, hydrationState, resultFor, onToggleTool, onHydrateTool, onResourceClick,
}: AssistantMessageTimelineProps) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      {items.map((item, index) => <TimelineItem key={`${message.id}-timeline-${index}`} item={item} index={index} props={{ message, items, isLive, expandedTools, hydrationState, resultFor, onToggleTool, onHydrateTool, onResourceClick }} />)}
    </div>
  );
}
