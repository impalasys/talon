export type ToolInvocationItem = {
  toolCallId: string;
  toolName: string;
  args: unknown;
  result?: unknown;
};

export type AssistantTimelineItem =
  | { type: 'text'; text: string }
  | {
      type: 'tool';
      toolCallId: string;
      toolName: string;
      args: unknown;
      result?: unknown;
    };

export type UsageSummary = {
  inputTokens?: number;
  outputTokens?: number;
  reasoningTokens?: number;
  totalTokens?: number;
};

function isActionStep(stepType: unknown): boolean {
  return stepType === 2 || stepType === 'STEP_TYPE_ACTION';
}

function isTokenStep(stepType: unknown): boolean {
  return stepType === 1 || stepType === 'STEP_TYPE_TOKEN';
}

function isObservationStep(stepType: unknown): boolean {
  return stepType === 3 || stepType === 'STEP_TYPE_OBSERVATION';
}

function isReasoningStep(stepType: unknown): boolean {
  return stepType === 6 || stepType === 'STEP_TYPE_REASONING';
}

function isUsageStep(stepType: unknown): boolean {
  return stepType === 7 || stepType === 'STEP_TYPE_USAGE';
}

function isErrorStep(stepType: unknown): boolean {
  return stepType === 5 || stepType === 'STEP_TYPE_ERROR';
}

function parseObjectPayload(payload: unknown): Record<string, unknown> {
  return payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
}

function parseJsonObject(payloadJson: unknown): Record<string, unknown> {
  if (typeof payloadJson !== 'string' || payloadJson.length === 0) return {};
  try {
    return parseObjectPayload(JSON.parse(payloadJson));
  } catch {
    return {};
  }
}

function parsePartPayload(part: any): Record<string, unknown> {
  if (!part || typeof part !== 'object') return {};
  const payload = part.payloadJson ?? part.payload_json;
  if (typeof payload === 'string') return parseJsonObject(payload);
  return parseObjectPayload(payload);
}

function partType(part: any): unknown {
  return part?.partType ?? part?.part_type ?? part?.type;
}

function partContent(part: any): string {
  if (typeof part?.text === 'string') return part.text;
  if (typeof part?.content === 'string') return part.content;
  return '';
}

function isTextPart(part: any): boolean {
  const type = partType(part);
  return type === 'text' || type === 1 || type === 'SESSION_MESSAGE_PART_TYPE_TEXT';
}

function isReasoningPart(part: any): boolean {
  const type = partType(part);
  return type === 'reasoning' || type === 2 || type === 'SESSION_MESSAGE_PART_TYPE_REASONING';
}

function isErrorPart(part: any): boolean {
  const type = partType(part);
  return type === 6 || type === 'SESSION_MESSAGE_PART_TYPE_ERROR';
}

function isToolCallPart(part: any): boolean {
  const type = partType(part);
  return type === 3 || type === 'SESSION_MESSAGE_PART_TYPE_TOOL_CALL';
}

function isToolResultPart(part: any): boolean {
  const type = partType(part);
  return type === 4 || type === 'SESSION_MESSAGE_PART_TYPE_TOOL_RESULT';
}

function appendTextToTimeline(
  timeline: AssistantTimelineItem[],
  chunk: string,
): AssistantTimelineItem[] {
  if (!chunk) return timeline;
  const nextTimeline = [...timeline];
  const lastItem = nextTimeline.at(-1);
  if (lastItem?.type === 'text') {
    nextTimeline[nextTimeline.length - 1] = {
      type: 'text',
      text: `${lastItem.text}${chunk}`,
    };
  } else {
    nextTimeline.push({ type: 'text', text: chunk });
  }
  return nextTimeline;
}

function upsertToolInTimeline(
  timeline: AssistantTimelineItem[],
  toolCallId: string,
  toolName: string,
  args: unknown,
  result?: unknown,
): AssistantTimelineItem[] {
  if (!toolCallId) return timeline;
  const nextTimeline = [...timeline];
  const index = nextTimeline.findIndex(
    (item) => item.type === 'tool' && item.toolCallId === toolCallId,
  );
  const previous =
    index >= 0 && nextTimeline[index]?.type === 'tool'
      ? (nextTimeline[index] as Extract<AssistantTimelineItem, { type: 'tool' }>)
      : null;
  const nextItem: Extract<AssistantTimelineItem, { type: 'tool' }> = {
    type: 'tool',
    toolCallId,
    toolName: toolName || previous?.toolName || 'tool',
    args: args ?? previous?.args ?? {},
    result: result ?? previous?.result,
  };
  if (index >= 0) {
    nextTimeline[index] = nextItem;
  } else {
    nextTimeline.push(nextItem);
  }
  return nextTimeline;
}

function legacyToolInvocationsFromParts(message: any): ToolInvocationItem[] {
  if (!Array.isArray(message?.parts)) return [];

  const toolInvocations = new Map<string, ToolInvocationItem>();
  for (const part of message.parts) {
    if (!part || typeof part !== 'object') continue;

    const payload = parsePartPayload(part);
    const toolCallId =
      typeof part.toolCallId === 'string'
        ? part.toolCallId
        : typeof payload.tool_call_id === 'string'
          ? payload.tool_call_id
          : '';
    if (!toolCallId) continue;

    const toolName =
      typeof part.toolName === 'string'
        ? part.toolName
        : typeof part.type === 'string' && part.type.startsWith('tool-')
          ? part.type.slice(5)
          : typeof part.name === 'string' && part.name
            ? part.name
            : 'tool';

    const previous = toolInvocations.get(toolCallId);
    toolInvocations.set(toolCallId, {
      toolCallId,
      toolName,
      args: 'input' in part ? part.input : payload.input ?? previous?.args ?? {},
      result:
        part.state === 'output-available'
          ? part.output
          : part.state === 'output-error'
            ? part.errorText
            : isToolResultPart(part)
              ? payload.output ?? payload.output_preview ?? partContent(part)
              : previous?.result,
    });
  }

  return Array.from(toolInvocations.values());
}

function timelineFromParts(message: any): AssistantTimelineItem[] {
  if (!Array.isArray(message?.parts)) return [];

  let timeline: AssistantTimelineItem[] = [];
  for (const part of message.parts) {
    if (!part || typeof part !== 'object') continue;

    if (isTextPart(part) || isErrorPart(part)) {
      timeline = appendTextToTimeline(timeline, partContent(part));
      continue;
    }

    const payload = parsePartPayload(part);
    const toolCallId =
      typeof part.toolCallId === 'string'
        ? part.toolCallId
        : typeof payload.tool_call_id === 'string'
          ? payload.tool_call_id
          : '';
    const toolName =
      typeof part.toolName === 'string'
        ? part.toolName
        : typeof part.type === 'string' && part.type.startsWith('tool-')
          ? part.type.slice(5)
          : typeof part.name === 'string' && part.name
            ? part.name
            : 'tool';

    if (isToolCallPart(part) || (typeof part.type === 'string' && part.type.startsWith('tool-'))) {
      timeline = upsertToolInTimeline(
        timeline,
        toolCallId,
        toolName,
        'input' in part ? part.input : payload.input ?? {},
        part.state === 'output-available'
          ? part.output
          : part.state === 'output-error'
            ? part.errorText
            : undefined,
      );
      continue;
    }

    if (isToolResultPart(part)) {
      timeline = upsertToolInTimeline(
        timeline,
        toolCallId,
        toolName,
        undefined,
        payload.output ?? payload.output_preview ?? partContent(part),
      );
    }
  }

  return timeline;
}

export function getMessageContent(message: any): string {
  if (Array.isArray(message?.parts)) {
    const content = message.parts
      .filter((part: any) => isTextPart(part) || isErrorPart(part))
      .map(partContent)
      .join('');
    if (content) return content;
  }
  return typeof message?.content === 'string' ? message.content : '';
}

export function getMessageReasoningContent(message: any): string {
  if (typeof message?.reasoningContent === 'string') {
    return message.reasoningContent;
  }

  if (!Array.isArray(message?.parts)) return '';
  return message.parts
    .filter(isReasoningPart)
    .map(partContent)
    .join('');
}

export function getMessageUsage(message: any): UsageSummary | null {
  if (message?.usage && typeof message.usage === 'object') {
    return message.usage as UsageSummary;
  }
  if (Array.isArray(message?.parts)) {
    const usagePart = message.parts.find(
      (part: any) => partType(part) === 5 || partType(part) === 'SESSION_MESSAGE_PART_TYPE_USAGE',
    );
    if (usagePart) {
      const payload = parsePartPayload(usagePart);
      return {
        inputTokens: typeof payload.input_tokens === 'number' ? payload.input_tokens : undefined,
        outputTokens: typeof payload.output_tokens === 'number' ? payload.output_tokens : undefined,
        reasoningTokens: typeof payload.reasoning_tokens === 'number' ? payload.reasoning_tokens : undefined,
        totalTokens: typeof payload.total_tokens === 'number' ? payload.total_tokens : undefined,
      };
    }
  }
  return null;
}

export function getMessageAssistantTimeline(message: any): AssistantTimelineItem[] {
  if (Array.isArray(message?.timeline) && message.timeline.length > 0) {
    return message.timeline as AssistantTimelineItem[];
  }

  const partTimeline = timelineFromParts(message);
  if (partTimeline.length > 0) return partTimeline;

  const toolInvocations = Array.isArray(message?.toolInvocations)
    ? (message.toolInvocations as ToolInvocationItem[])
    : legacyToolInvocationsFromParts(message);
  const content = getMessageContent(message);
  const fallbackTimeline: AssistantTimelineItem[] = [];
  if (content) {
    fallbackTimeline.push({ type: 'text', text: content });
  }
  for (const tool of toolInvocations) {
    fallbackTimeline.push({
      type: 'tool',
      toolCallId: tool.toolCallId,
      toolName: tool.toolName,
      args: tool.args,
      result: tool.result,
    });
  }
  return fallbackTimeline;
}

export function getMessageToolInvocations(message: any): ToolInvocationItem[] {
  const timeline = getMessageAssistantTimeline(message);
  const toolInvocations = timeline
    .filter((item): item is Extract<AssistantTimelineItem, { type: 'tool' }> => item.type === 'tool')
    .map((item) => ({
      toolCallId: item.toolCallId,
      toolName: item.toolName,
      args: item.args,
      result: item.result,
    }));
  return toolInvocations.length > 0
    ? toolInvocations
    : Array.isArray(message?.toolInvocations)
      ? (message.toolInvocations as ToolInvocationItem[])
      : legacyToolInvocationsFromParts(message);
}

export function normalizeMessageRole(role: unknown): 'user' | 'assistant' | 'system' {
  if (role === 1 || role === 'ROLE_USER') return 'user';
  if (role === 2 || role === 'ROLE_ASSISTANT') return 'assistant';
  if (role === 3 || role === 'ROLE_SYSTEM') return 'system';
  return 'assistant';
}

export function isPlaceholderBootMessage(messages: any[]) {
  return (
    messages.length === 1 &&
    messages[0]?.role === 'system' &&
    getMessageContent(messages[0]) === 'Talon runtime initialized.'
  );
}

function buildReasoningFromSteps(steps: any[] | undefined): Map<string, string> {
  const byMessage = new Map<string, string>();
  if (!Array.isArray(steps)) return byMessage;

  for (const step of steps) {
    const messageId = step?.messageId;
    if (!messageId || !isReasoningStep(step?.stepType) || typeof step?.content !== 'string') continue;
    byMessage.set(messageId, `${byMessage.get(messageId) ?? ''}${step.content}`);
  }

  return byMessage;
}

function buildUsageFromSteps(steps: any[] | undefined): Map<string, UsageSummary> {
  const byMessage = new Map<string, UsageSummary>();
  if (!Array.isArray(steps)) return byMessage;

  for (const step of steps) {
    const messageId = step?.messageId;
    if (!messageId || !isUsageStep(step?.stepType)) continue;

    const payload = parseJsonObject(step.payloadJson);
    byMessage.set(messageId, {
      inputTokens: typeof payload.input_tokens === 'number' ? payload.input_tokens : undefined,
      outputTokens: typeof payload.output_tokens === 'number' ? payload.output_tokens : undefined,
      reasoningTokens: typeof payload.reasoning_tokens === 'number' ? payload.reasoning_tokens : undefined,
      totalTokens: typeof payload.total_tokens === 'number' ? payload.total_tokens : undefined,
    });
  }

  return byMessage;
}

function buildAssistantTimelineFromSteps(steps: any[] | undefined): Map<string, AssistantTimelineItem[]> {
  const byMessage = new Map<string, AssistantTimelineItem[]>();
  if (!Array.isArray(steps)) return byMessage;

  for (const step of steps) {
    const messageId = step?.messageId;
    if (!messageId) continue;

    const timeline = byMessage.get(messageId) ?? [];

    if (typeof step?.content === 'string' && step.content && isTokenStep(step.stepType)) {
      byMessage.set(messageId, appendTextToTimeline(timeline, step.content));
      continue;
    }

    if (isErrorStep(step?.stepType)) {
      byMessage.set(
        messageId,
        appendTextToTimeline(timeline, typeof step?.content === 'string' && step.content ? step.content : 'Stream error'),
      );
      continue;
    }

    if (isActionStep(step?.stepType)) {
      const payload = parseJsonObject(step.payloadJson);
      const toolCallId = typeof payload.tool_call_id === 'string' ? payload.tool_call_id : '';
      byMessage.set(
        messageId,
        upsertToolInTimeline(
          timeline,
          toolCallId,
          step.name || 'tool',
          payload.input ?? {},
        ),
      );
      continue;
    }

    if (isObservationStep(step?.stepType)) {
      const payload = parseJsonObject(step.payloadJson);
      const toolCallId = typeof payload.tool_call_id === 'string' ? payload.tool_call_id : '';
      byMessage.set(
        messageId,
        upsertToolInTimeline(
          timeline,
          toolCallId,
          step.name || 'tool',
          undefined,
          payload.output ?? step.content,
        ),
      );
    }
  }

  return byMessage;
}

export function hydrateMessagesWithSteps(messages: any[], steps: any[] | undefined) {
  const timelineByMessage = buildAssistantTimelineFromSteps(steps);
  const reasoningByMessage = buildReasoningFromSteps(steps);
  const usageByMessage = buildUsageFromSteps(steps);

  return messages.map((message) => {
    if (message.role !== 'assistant') return message;
    const timeline = timelineByMessage.get(message.id);
    const reasoningContent = reasoningByMessage.get(message.id);
    const usage = usageByMessage.get(message.id);
    if ((!timeline || timeline.length === 0) && !reasoningContent && !usage) {
      return message;
    }
    return {
      ...message,
      ...(timeline && timeline.length > 0 ? { timeline } : {}),
      ...(reasoningContent ? { reasoningContent } : {}),
      ...(usage ? { usage } : {}),
    };
  });
}

export function ensureAssistantMessage(messages: any[], messageId: string) {
  if (messages.some((message) => message.id === messageId)) {
    return messages;
  }
  return [
    ...messages,
    {
      id: messageId,
      role: 'assistant',
      content: '',
      parts: [{ type: 'text', text: '' }],
      reasoningContent: '',
      timeline: [],
    },
  ];
}

export function reconcileAssistantMessageId(messages: any[], fromMessageId: string, toMessageId: string) {
  if (!fromMessageId || !toMessageId || fromMessageId === toMessageId) {
    return messages;
  }

  const fromIndex = messages.findIndex((message) => message.id === fromMessageId);
  if (fromIndex < 0) {
    return ensureAssistantMessage(messages, toMessageId);
  }

  const toIndex = messages.findIndex((message) => message.id === toMessageId);
  const nextMessages = [...messages];

  if (toIndex >= 0) {
    const fromMessage = nextMessages[fromIndex];
    const toMessage = nextMessages[toIndex];
    const mergedText = `${getMessageContent(toMessage)}${getMessageContent(fromMessage)}`;
    const mergedReasoning = `${getMessageReasoningContent(toMessage)}${getMessageReasoningContent(fromMessage)}`;
    const mergedTimeline = [
      ...getMessageAssistantTimeline(toMessage),
      ...getMessageAssistantTimeline(fromMessage),
    ];
    nextMessages[toIndex] = {
      ...toMessage,
      content: mergedText,
      parts: [{ type: 'text', text: mergedText }],
      reasoningContent: mergedReasoning,
      timeline: mergedTimeline,
      usage: toMessage.usage ?? fromMessage.usage,
    };
    nextMessages.splice(fromIndex, 1);
    return nextMessages;
  }

  nextMessages[fromIndex] = {
    ...nextMessages[fromIndex],
    id: toMessageId,
  };
  return nextMessages;
}

export function appendAssistantText(messages: any[], messageId: string, chunk: string) {
  const existingIndex = messages.findIndex((message) => message.id === messageId);
  const nextMessages = existingIndex >= 0 ? [...messages] : ensureAssistantMessage(messages, messageId);
  const assistantIndex =
    existingIndex >= 0 ? existingIndex : nextMessages.findIndex((message) => message.id === messageId);
  const current = nextMessages[assistantIndex];
  const nextContent = `${getMessageContent(current)}${chunk}`;
  nextMessages[assistantIndex] = {
    ...current,
    content: nextContent,
    parts: [{ type: 'text', text: nextContent }],
    timeline: appendTextToTimeline(getMessageAssistantTimeline(current), chunk),
  };
  return nextMessages;
}

export function appendAssistantReasoning(messages: any[], messageId: string, chunk: string) {
  const existingIndex = messages.findIndex((message) => message.id === messageId);
  const nextMessages = existingIndex >= 0 ? [...messages] : ensureAssistantMessage(messages, messageId);
  const assistantIndex =
    existingIndex >= 0 ? existingIndex : nextMessages.findIndex((message) => message.id === messageId);
  const current = nextMessages[assistantIndex];
  nextMessages[assistantIndex] = {
    ...current,
    reasoningContent: `${getMessageReasoningContent(current)}${chunk}`,
  };
  return nextMessages;
}

export function applyToolInvocationToMessages(
  messages: any[],
  toolCallId: string,
  toolName: string,
  args: unknown,
  result?: unknown,
  assistantMessageId?: string,
) {
  const lastAssistantIndex = assistantMessageId
    ? messages.findIndex((message) => message.id === assistantMessageId)
    : [...messages]
        .map((message, index) => ({ message, index }))
        .reverse()
        .find(({ message }) => message.role === 'assistant')?.index;

  if (lastAssistantIndex == null) return messages;

  const current = messages[lastAssistantIndex];
  const nextMessages = [...messages];
  nextMessages[lastAssistantIndex] = {
    ...current,
    timeline: upsertToolInTimeline(
      getMessageAssistantTimeline(current),
      toolCallId,
      toolName,
      args,
      result,
    ),
  };
  return nextMessages;
}

export function applyUsageToMessages(messages: any[], messageId: string, usage: UsageSummary) {
  const existingIndex = messages.findIndex((message) => message.id === messageId);
  const nextMessages = existingIndex >= 0 ? [...messages] : ensureAssistantMessage(messages, messageId);
  const assistantIndex =
    existingIndex >= 0 ? existingIndex : nextMessages.findIndex((message) => message.id === messageId);
  const current = nextMessages[assistantIndex];
  nextMessages[assistantIndex] = {
    ...current,
    usage,
  };
  return nextMessages;
}

export function formatUsageSummary(usage: UsageSummary | null) {
  if (!usage) return '';
  const parts = [
    typeof usage.reasoningTokens === 'number' ? `${usage.reasoningTokens} reasoning` : '',
    typeof usage.outputTokens === 'number' ? `${usage.outputTokens} output` : '',
    typeof usage.inputTokens === 'number' ? `${usage.inputTokens} input` : '',
  ].filter(Boolean);
  const total = typeof usage.totalTokens === 'number' ? `${usage.totalTokens} total` : '';
  return [parts.join(' • '), total].filter(Boolean).join(' • ');
}
