// @ts-ignore The Node strip-types test runner requires explicit .ts resolution.
import { getMessageContent, hydrateMessagesWithSteps, normalizeMessageRole, type CopilotMessage } from "../lib/chatTimeline.ts";

export type HistoryState = {
  messages: CopilotMessage[];
  hasMoreOlder: boolean;
  beforeMessageId: string | null;
};

export type SessionHistoryPage = HistoryState & {
  state: string;
  hasMore: boolean;
  nextBeforeMessageId: string | null;
};

export function normalizeMessageLabels(labels: unknown): Record<string, string> | undefined {
  if (!labels || typeof labels !== "object" || Array.isArray(labels)) return undefined;
  const entries = Object.entries(labels as Record<string, unknown>)
    .filter((entry): entry is [string, string] => typeof entry[1] === "string");
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}

export function historyMessageTimestamp(message: Pick<CopilotMessage, "createdAt"> | undefined): number | null {
  const value = message?.createdAt;
  if (value == null) return null;
  const numeric = typeof value === "bigint" ? Number(value) : Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) return null;
  if (numeric >= 1e15) return Math.trunc(numeric / 1000);
  if (numeric >= 1e12) return Math.trunc(numeric);
  if (numeric >= 1e9) return Math.trunc(numeric * 1000);
  return null;
}

export function isLocalMessageId(id: string): boolean {
  return id.startsWith("local-") || id.startsWith("msg-");
}

export function canCompareCanonicalMessageIds(left: string, right: string): boolean {
  return !isLocalMessageId(left) && !isLocalMessageId(right);
}

export function historyItemsFromResponse(response: any): Array<{ message?: any; steps?: any[] }> {
  if (Array.isArray(response?.items)) return response.items;
  if (Array.isArray(response?.messages)) {
    const stepsByMessage = new Map<string, any[]>();
    for (const step of response.steps || []) {
      const messageId = step?.messageId ?? step?.message_id;
      if (!messageId) continue;
      stepsByMessage.set(messageId, [...(stepsByMessage.get(messageId) ?? []), step]);
    }
    return response.messages.map((message: any) => ({
      message,
      steps: stepsByMessage.get(message?.id) ?? [],
    }));
  }
  return [];
}

export function normalizeRawSessionMessage(message: any): CopilotMessage {
  if (typeof message?.id !== "string" || message.id.length === 0) {
    throw new Error("Session history messages must include a canonical id.");
  }
  return {
    id: message.id,
    role: normalizeMessageRole(message?.role),
    content: getMessageContent(message),
    labels: normalizeMessageLabels(message?.labels),
    parts: Array.isArray(message?.parts) ? message.parts : undefined,
    createdAt: message?.createdAt ?? message?.created_at,
  };
}

export function normalizeHistoryPage(response: any): SessionHistoryPage {
  const items = historyItemsFromResponse(response);
  const rawMessages = items
    .map((item) => item?.message)
    .filter(Boolean)
    .map((message: any) => normalizeRawSessionMessage(message));
  const steps = items.flatMap((item) => item?.steps || []);
  const messages = hydrateMessagesWithSteps(rawMessages, steps);
  const nextBeforeMessageId = typeof response?.nextBeforeMessageId === "string"
    ? response.nextBeforeMessageId
    : typeof response?.next_before_message_id === "string"
      ? response.next_before_message_id
      : null;
  const hasMoreOlder = Boolean(response?.hasMore ?? response?.has_more);
  return {
    messages,
    state: typeof response?.state === "string" ? response.state : "IDLE",
    hasMore: hasMoreOlder,
    nextBeforeMessageId,
    hasMoreOlder,
    beforeMessageId: nextBeforeMessageId,
  };
}

export function mergeNewestCanonicalPage(
  existingMessages: CopilotMessage[],
  newestPageMessages: CopilotMessage[],
  options: { preserveOptimistic?: boolean } = {},
): CopilotMessage[] {
  if (newestPageMessages.length === 0) return existingMessages;
  const newestIds = new Set(newestPageMessages.map((message) => message.id));
  const oldestPageId = newestPageMessages[0]?.id;
  const newestPageId = newestPageMessages[newestPageMessages.length - 1]?.id;
  const oldestPageTimestamp = historyMessageTimestamp(newestPageMessages[0]);
  const newestPageTimestamp = historyMessageTimestamp(newestPageMessages[newestPageMessages.length - 1]);
  const optimisticMessages = options.preserveOptimistic
    ? existingMessages.filter((message) => isLocalMessageId(message.id))
    : [];
  const preservedOlderMessages = existingMessages.filter((message) => {
    if (message.id === "1") return true;
    if (isLocalMessageId(message.id)) return false;
    if (newestIds.has(message.id)) return false;
    const messageTimestamp = historyMessageTimestamp(message);
    if (messageTimestamp !== null && oldestPageTimestamp !== null) return messageTimestamp < oldestPageTimestamp;
    return Boolean(oldestPageId && canCompareCanonicalMessageIds(message.id, oldestPageId) && message.id < oldestPageId);
  });
  const preservedNewerMessages = existingMessages.filter((message) => {
    if (message.id === "1" || isLocalMessageId(message.id) || newestIds.has(message.id)) return false;
    const messageTimestamp = historyMessageTimestamp(message);
    if (messageTimestamp !== null && newestPageTimestamp !== null) return messageTimestamp > newestPageTimestamp;
    return Boolean(newestPageId && canCompareCanonicalMessageIds(message.id, newestPageId) && message.id > newestPageId);
  });
  const dedupedMessages = new Map<string, CopilotMessage>();
  for (const message of [...preservedOlderMessages, ...optimisticMessages, ...newestPageMessages, ...preservedNewerMessages]) {
    dedupedMessages.set(message.id, message);
  }
  return Array.from(dedupedMessages.values());
}

export function validateCursorAdvances(previous: string | null, next: string | null): boolean {
  if (!next) return false;
  if (!previous) return true;
  return next !== previous;
}

export function historyStateFromPage(page: SessionHistoryPage): HistoryState {
  return {
    messages: page.messages,
    hasMoreOlder: page.hasMoreOlder,
    beforeMessageId: page.beforeMessageId,
  };
}
