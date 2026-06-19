import {
  appendAssistantText,
  appendAssistantReasoning,
  applyToolInvocationToMessages,
  applyUsageToMessages,
  ensureAssistantMessage,
  formatUsageSummary,
  getMessageAssistantTimeline,
  getMessageContent,
  getMessageReasoningContent,
  getMessageToolInvocations,
  getMessageUsage,
  hydrateMessagesWithSteps,
  isPlaceholderBootMessage,
  normalizeMessageRole,
  reconcileAssistantMessageId,
} from "./chatTimeline";

describe("hydrateMessagesWithSteps", () => {
  it("interleaves tool calls with text in step order", () => {
    const messages = [
      {
        id: "assistant-1",
        role: "assistant",
        content: "Let me check that. I found the answer.",
      },
    ];
    const steps = [
      {
        messageId: "assistant-1",
        stepType: 1,
        content: "Let me check that. ",
        payloadJson: "",
      },
      {
        messageId: "assistant-1",
        stepType: 2,
        name: "knowledge_search",
        payloadJson: JSON.stringify({
          tool_call_id: "call-1",
          input: { query: "docs.example.com" },
        }),
      },
      {
        messageId: "assistant-1",
        stepType: 3,
        name: "knowledge_search",
        content: "",
        payloadJson: JSON.stringify({
          tool_call_id: "call-1",
          output: { matches: 1 },
        }),
      },
      {
        messageId: "assistant-1",
        stepType: 1,
        content: "I found the answer.",
        payloadJson: "",
      },
    ];

    const hydrated = hydrateMessagesWithSteps(messages, steps);
    expect(getMessageAssistantTimeline(hydrated[0])).toEqual([
      { type: "text", text: "Let me check that. " },
      {
        type: "tool",
        toolCallId: "call-1",
        toolName: "knowledge_search",
        args: { query: "docs.example.com" },
        result: { matches: 1 },
      },
      { type: "text", text: "I found the answer." },
    ]);
  });

  it("hydrates error steps into the assistant timeline", () => {
    const hydrated = hydrateMessagesWithSteps(
      [{ id: "assistant-1", role: "assistant", content: "" }],
      [
        {
          messageId: "assistant-1",
          stepType: "STEP_TYPE_ERROR",
          content: "Error: provider overloaded",
          payloadJson: "",
        },
      ],
    );

    expect(getMessageAssistantTimeline(hydrated[0])).toEqual([
      { type: "text", text: "Error: provider overloaded" },
    ]);
  });
});

describe("live timeline helpers", () => {
  it("keeps later text after the tool card", () => {
    let messages: any[] = [
      { id: "assistant-1", role: "assistant", content: "", timeline: [] },
    ];

    messages = appendAssistantText(messages, "assistant-1", "Segment 1 ");
    messages = applyToolInvocationToMessages(
      messages,
      "call-1",
      "knowledge_search",
      { query: "docs.example.com" },
      { ok: true },
      "assistant-1",
    );
    messages = appendAssistantText(messages, "assistant-1", "Segment 2");

    expect(getMessageAssistantTimeline(messages[0])).toEqual([
      { type: "text", text: "Segment 1 " },
      {
        type: "tool",
        toolCallId: "call-1",
        toolName: "knowledge_search",
        args: { query: "docs.example.com" },
        result: { ok: true },
      },
      { type: "text", text: "Segment 2" },
    ]);
  });

  it("ignores empty text deltas and empty tool ids", () => {
    let messages: any[] = [
      { id: "assistant-1", role: "assistant", content: "", timeline: [] },
    ];

    messages = appendAssistantText(messages, "assistant-1", "");
    messages = applyToolInvocationToMessages(messages, "", "knowledge_search", {}, undefined, "assistant-1");

    expect(getMessageAssistantTimeline(messages[0])).toEqual([]);
  });

  it("handles non-array message parts without crashing", () => {
    const message = {
      id: "assistant-1",
      role: "assistant",
      content: "Legacy fallback",
      parts: { type: "text", text: "not an array" },
    };

    expect(getMessageContent(message)).toBe("Legacy fallback");
    expect(getMessageReasoningContent(message)).toBe("");
    expect(getMessageUsage(message)).toBeNull();
    expect(getMessageAssistantTimeline(message)).toEqual([
      { type: "text", text: "Legacy fallback" },
    ]);
  });

  it("can append reasoning, usage, and reconcile placeholder ids", () => {
    let messages: any[] = ensureAssistantMessage([], "temp-id");
    messages = appendAssistantText(messages, "temp-id", "Working ");
    messages = appendAssistantReasoning(messages, "temp-id", "Think once. ");
    messages = applyUsageToMessages(messages, "temp-id", { reasoningTokens: 6 });
    messages = reconcileAssistantMessageId(messages, "temp-id", "assistant-1");

    expect(messages[0].id).toBe("assistant-1");
    expect(getMessageContent(messages[0])).toBe("Working ");
    expect(getMessageReasoningContent(messages[0])).toBe("Think once. ");
    expect(getMessageUsage(messages[0])).toEqual({ reasoningTokens: 6 });
  });

  it("preserves existing tool cards when a result arrives without a new args payload", () => {
    let messages: any[] = [
      { id: "assistant-1", role: "assistant", content: "", timeline: [] },
    ];

    messages = applyToolInvocationToMessages(
      messages,
      "call-1",
      "knowledge_search",
      { query: "docs" },
      undefined,
      "assistant-1",
    );
    messages = applyToolInvocationToMessages(
      messages,
      "call-1",
      "",
      undefined,
      "done",
      "assistant-1",
    );

    expect(getMessageAssistantTimeline(messages[0])).toEqual([
      {
        type: "tool",
        toolCallId: "call-1",
        toolName: "knowledge_search",
        args: { query: "docs" },
        result: "done",
      },
    ]);
  });

  it("merges placeholder and canonical assistant ids", () => {
    const merged = reconcileAssistantMessageId(
      [
        { id: "canonical", role: "assistant", content: "A", timeline: [{ type: "text", text: "A" }] },
        { id: "temp", role: "assistant", content: "B", timeline: [{ type: "text", text: "B" }] },
      ],
      "temp",
      "canonical",
    );

    expect(merged).toHaveLength(1);
    expect(getMessageContent(merged[0])).toBe("AB");
    expect(getMessageAssistantTimeline(merged[0])).toEqual([
      { type: "text", text: "A" },
      { type: "text", text: "B" },
    ]);
  });
});

describe("fallback readers", () => {
  it("reads legacy content, parts, and tool invocations", () => {
    const message = {
      role: "assistant",
      parts: [
        { type: "text", text: "Hello " },
        { type: "text", text: "world" },
        {
          type: "tool-knowledge_search",
          toolCallId: "call-1",
          input: { query: "docs" },
          state: "output-available",
          output: "done",
        },
        { type: "reasoning", text: "step 1" },
      ],
    };

    expect(getMessageContent(message)).toBe("Hello world");
    expect(getMessageReasoningContent(message)).toBe("step 1");
    expect(getMessageToolInvocations(message)).toEqual([
      {
        toolCallId: "call-1",
        toolName: "knowledge_search",
        args: { query: "docs" },
        result: "done",
      },
    ]);
  });

  it("reads canonical Talon message parts instead of deprecated message content", () => {
    const message = {
      role: "assistant",
      content: "stale legacy content",
      parts: [
        { partType: 1, content: "Hello " },
        { part_type: "SESSION_MESSAGE_PART_TYPE_TEXT", content: "world" },
        { partType: 2, content: "reasoning step" },
        {
          partType: 3,
          name: "knowledge_search",
          payloadJson: JSON.stringify({
            tool_call_id: "call-1",
            input: { query: "docs" },
          }),
        },
        {
          part_type: "SESSION_MESSAGE_PART_TYPE_TOOL_RESULT",
          name: "knowledge_search",
          content: "done",
          payload_json: JSON.stringify({
            tool_call_id: "call-1",
            output: "done",
          }),
        },
      ],
    };

    expect(getMessageContent(message)).toBe("Hello world");
    expect(getMessageReasoningContent(message)).toBe("reasoning step");
    expect(getMessageToolInvocations(message)).toEqual([
      {
        toolCallId: "call-1",
        toolName: "knowledge_search",
        args: { query: "docs" },
        result: "done",
      },
    ]);
    expect(getMessageAssistantTimeline(message)).toEqual([
      { type: "text", text: "Hello world" },
      {
        type: "tool",
        toolCallId: "call-1",
        toolName: "knowledge_search",
        args: { query: "docs" },
        result: "done",
      },
    ]);
  });

  it("reads canonical error and usage parts", () => {
    const message = {
      role: "assistant",
      parts: [
        { part_type: "SESSION_MESSAGE_PART_TYPE_ERROR", content: "Provider failed" },
        {
          partType: 5,
          payloadJson: JSON.stringify({
            input_tokens: 10,
            output_tokens: 3,
            reasoning_tokens: 2,
            total_tokens: 15,
          }),
        },
      ],
    };

    expect(getMessageContent(message)).toBe("Provider failed");
    expect(getMessageAssistantTimeline(message)).toEqual([
      { type: "text", text: "Provider failed" },
    ]);
    expect(getMessageUsage(message)).toEqual({
      inputTokens: 10,
      outputTokens: 3,
      reasoningTokens: 2,
      totalTokens: 15,
    });
    expect(getMessageUsage({ parts: [] })).toBeNull();
  });

  it("renders permission request parts as timeline work items", () => {
    const message = {
      role: "assistant",
      parts: [
        {
          partType: "SESSION_MESSAGE_PART_TYPE_REQUEST_PERMISSION",
          payloadJson: JSON.stringify({
            requestId: "perm-1",
            action: "terminal",
            request: { command: "npm test" },
          }),
        },
        {
          partType: 12,
          payloadJson: JSON.stringify({
            requestId: "perm-1",
            outcome: { outcome: "selected", optionId: "approved" },
          }),
        },
      ],
    };

    expect(getMessageAssistantTimeline(message)).toEqual([
      {
        type: "tool",
        toolCallId: "perm-1",
        toolName: "request_permission",
        args: { command: "npm test" },
        result: { outcome: "selected", optionId: "approved" },
      },
    ]);
  });

  it("reads camelCase tool and usage fields from part payloads", () => {
    const message = {
      role: "assistant",
      parts: [
        {
          partType: 3,
          payloadJson: JSON.stringify({
            toolCallId: "call-camel",
            input: { query: "ops" },
          }),
        },
        {
          partType: 4,
          payloadJson: JSON.stringify({
            toolCallId: "call-camel",
            outputPreview: "preview result",
          }),
        },
        {
          partType: 5,
          payloadJson: JSON.stringify({
            inputTokens: 10,
            outputTokens: 3,
            reasoningTokens: 2,
            totalTokens: 15,
          }),
        },
      ],
    };

    expect(getMessageAssistantTimeline(message)).toEqual([
      {
        type: "tool",
        toolCallId: "call-camel",
        toolName: "tool",
        args: { query: "ops" },
        result: "preview result",
      },
    ]);
    expect(getMessageUsage(message)).toEqual({
      inputTokens: 10,
      outputTokens: 3,
      reasoningTokens: 2,
      totalTokens: 15,
    });
  });

  it("keeps AI SDK tool parts interleaved with text", () => {
    const message = {
      role: "assistant",
      parts: [
        { type: "text", text: "Before " },
        {
          type: "tool-search",
          toolCallId: "call-2",
          input: { query: "ops" },
          state: "output-error",
          errorText: "denied",
        },
        { type: "text", text: "after" },
      ],
    };

    expect(getMessageAssistantTimeline(message)).toEqual([
      { type: "text", text: "Before " },
      {
        type: "tool",
        toolCallId: "call-2",
        toolName: "search",
        args: { query: "ops" },
        result: "denied",
      },
      { type: "text", text: "after" },
    ]);
  });

  it("falls back to text then tools when no timeline exists", () => {
    const message = {
      role: "assistant",
      content: "Final answer",
      toolInvocations: [
        { toolCallId: "call-1", toolName: "knowledge_search", args: { query: "docs" } },
      ],
    };

    expect(getMessageAssistantTimeline(message)).toEqual([
      { type: "text", text: "Final answer" },
      {
        type: "tool",
        toolCallId: "call-1",
        toolName: "knowledge_search",
        args: { query: "docs" },
      },
    ]);
  });

  it("handles no-op assistant id reconciliation and implicit tool targets", () => {
    const messages = [
      { id: "assistant-1", role: "assistant", content: "A" },
    ];

    expect(reconcileAssistantMessageId(messages, "", "assistant-2")).toBe(messages);
    expect(reconcileAssistantMessageId(messages, "missing", "assistant-2")).toEqual([
      ...messages,
      {
        id: "assistant-2",
        role: "assistant",
        content: "",
        parts: [{ type: "text", text: "" }],
        reasoningContent: "",
        timeline: [],
      },
    ]);

    const withTool = applyToolInvocationToMessages(messages, "call-3", "lookup", { id: 1 });
    expect(getMessageAssistantTimeline(withTool[0])).toEqual([
      { type: "text", text: "A" },
      {
        type: "tool",
        toolCallId: "call-3",
        toolName: "lookup",
        args: { id: 1 },
      },
    ]);
  });
});

describe("metadata helpers", () => {
  it("normalizes roles and placeholder boot messages", () => {
    expect(normalizeMessageRole(1)).toBe("user");
    expect(normalizeMessageRole("ROLE_ASSISTANT")).toBe("assistant");
    expect(normalizeMessageRole("ROLE_SYSTEM")).toBe("system");
    expect(normalizeMessageRole("unknown")).toBe("assistant");

    expect(
      isPlaceholderBootMessage([
        { role: "system", content: "Talon runtime initialized." },
      ]),
    ).toBe(true);
    expect(isPlaceholderBootMessage([{ role: "system", content: "something else" }])).toBe(false);
  });

  it("hydrates reasoning and usage without changing non-assistant messages", () => {
    const messages = [
      { id: "user-1", role: "user", content: "hello" },
      { id: "assistant-1", role: "assistant", content: "hello back" },
    ];
    const steps = [
      {
        messageId: "assistant-1",
        stepType: "STEP_TYPE_REASONING",
        content: "reason one ",
        payloadJson: "",
      },
      {
        messageId: "assistant-1",
        stepType: "STEP_TYPE_USAGE",
        content: "",
        payloadJson: JSON.stringify({
          input_tokens: 4,
          output_tokens: 5,
          reasoning_tokens: 6,
          total_tokens: 15,
        }),
      },
    ];

    const hydrated = hydrateMessagesWithSteps(messages, steps);
    expect(hydrated[0]).toEqual(messages[0]);
    expect(getMessageReasoningContent(hydrated[1])).toBe("reason one ");
    expect(getMessageUsage(hydrated[1])).toEqual({
      inputTokens: 4,
      outputTokens: 5,
      reasoningTokens: 6,
      totalTokens: 15,
    });
  });

  it("hydrates camelCase tool and usage fields from steps", () => {
    const messages = [
      { id: "assistant-1", role: "assistant", content: "" },
    ];
    const steps = [
      {
        messageId: "assistant-1",
        stepType: "STEP_TYPE_ACTION",
        name: "knowledge_search",
        payloadJson: JSON.stringify({
          toolCallId: "call-camel",
          input: { query: "docs" },
        }),
      },
      {
        messageId: "assistant-1",
        stepType: "STEP_TYPE_OBSERVATION",
        name: "knowledge_search",
        payloadJson: JSON.stringify({
          toolCallId: "call-camel",
          outputPreview: "docs found",
        }),
      },
      {
        messageId: "assistant-1",
        stepType: "STEP_TYPE_USAGE",
        payloadJson: JSON.stringify({
          inputTokens: 4,
          outputTokens: 5,
          reasoningTokens: 6,
          totalTokens: 15,
        }),
      },
    ];

    const hydrated = hydrateMessagesWithSteps(messages, steps);
    expect(getMessageAssistantTimeline(hydrated[0])).toEqual([
      {
        type: "tool",
        toolCallId: "call-camel",
        toolName: "knowledge_search",
        args: { query: "docs" },
        result: "docs found",
      },
    ]);
    expect(getMessageUsage(hydrated[0])).toEqual({
      inputTokens: 4,
      outputTokens: 5,
      reasoningTokens: 6,
      totalTokens: 15,
    });
  });

  it("ignores malformed payloads and blank chunks without crashing", () => {
    const hydrated = hydrateMessagesWithSteps(
      [{ id: "assistant-1", role: "assistant", content: "" }],
      [
        {
          messageId: "assistant-1",
          stepType: "STEP_TYPE_ACTION",
          name: "knowledge_search",
          payloadJson: "{not-json}",
        },
        {
          messageId: "assistant-1",
          stepType: "STEP_TYPE_TOKEN",
          content: "",
          payloadJson: "",
        },
      ],
    );

    expect(getMessageAssistantTimeline(hydrated[0])).toEqual([]);
  });
});

describe("formatUsageSummary", () => {
  it("formats reasoning and totals in a stable order", () => {
    expect(
      formatUsageSummary({
        reasoningTokens: 6,
        outputTokens: 10,
        inputTokens: 12,
        totalTokens: 28,
      }),
    ).toBe("6 reasoning • 10 output • 12 input • 28 total");
  });

  it("returns an empty string when usage is missing", () => {
    expect(formatUsageSummary(null)).toBe("");
  });
});
