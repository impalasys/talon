import React, { useEffect } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { TalonSession, type GatewayClientLike, type TalonSessionProps } from "./TalonSession";

const fixedMessages = [
  {
    id: "018f4b50-8cc0-7000-8000-000000000001",
    role: "ROLE_USER",
    parts: [{ type: "text", text: "Summarize the latest incident notes and identify the next owner." }],
    createdAt: "2026-06-05T16:15:00.000Z",
  },
  {
    id: "018f4b50-a234-7000-8000-000000000002",
    role: "ROLE_ASSISTANT",
    parts: [
      {
        type: "reasoning",
        text: "I need to inspect the latest incident notes, identify the active mitigation owner, and separate rollback validation from release readiness.",
      },
      {
        type: "tool-getIncidentId",
        toolCallId: "get-incident-id",
        toolName: "getIncidentId",
        input: {
          alias: "latest",
        },
        state: "output-available",
        output: {
          id: "inc-7429"
        },
      },
      {
        type: "tool-searchIncidentNotes",
        toolCallId: "call-incident-notes",
        toolName: "searchIncidentNotes",
        input: {
          incidentId: "inc-7429",
          limit: 3,
        },
        state: "output-available",
        output: {
          latestNoteId: "note-18",
          summary: "Deployment alert is scoped to ingestion; rollback validation assigned to Mia; Ravi is monitoring queue drain.",
        },
      },
      {
        type: "text",
        text: "The deployment alert is isolated to the ingestion worker. Mia owns rollback validation, and Ravi is checking the queue drain rate before the next release window.",
      },
      {
        type: "SESSION_MESSAGE_PART_TYPE_USAGE",
        payloadJson: JSON.stringify({
          input_tokens: 842,
          output_tokens: 42,
          reasoning_tokens: 96,
          total_tokens: 980,
        }),
      },
    ],
    createdAt: "2026-06-05T16:15:11.000Z",
  },
  {
    id: "018f4b52-4444-7000-8000-000000000003",
    role: "ROLE_USER",
    parts: [{ type: "text", text: "Draft a concise update for the launch channel." }],
    createdAt: "2026-06-05T16:16:05.000Z",
  },
  {
    id: "018f4b52-9000-7000-8000-000000000004",
    role: "ROLE_ASSISTANT",
    parts: [
      {
        type: "text",
        text: "Launch update: ingestion is healthy after the rollback guardrail, queue depth is trending down, and the team will hold the next release until validation completes.",
      },
    ],
    createdAt: "2026-06-05T16:16:18.000Z",
  },
];

const gatewayClient: GatewayClientLike = {
  createSession: async () => ({ sessionId: "storybook-session" }),
  listSessionMessages: async () => ({
    messages: fixedMessages,
    hasMore: false,
    state: "IDLE",
  }),
};

const mockImageUpload: TalonSessionProps["onImageUpload"] = async ({ file, namespace, agent, sessionId }) => ({
  key: `${namespace}/${agent}/${sessionId}/uploads/${file.name}`,
  mediaType: file.type || "image/png",
  sizeBytes: file.size,
  filename: file.name,
});

const streamingPrompt = "Summarize the latest incident notes and identify the next owner.";
const streamingAssistantMessage = fixedMessages[1];
const originalFetch = globalThis.fetch;

function uiStreamLine(code: string, value: unknown) {
  return `${code}:${JSON.stringify(value)}\n`;
}

function delay(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function createStreamingResponse(signal?: AbortSignal | null) {
  const encoder = new TextEncoder();
  const parts = Array.isArray(streamingAssistantMessage.parts) ? streamingAssistantMessage.parts : [];
  const reasoningPart = parts.find((part: any) => part?.type === "reasoning") as any;
  const toolParts = parts.filter((part: any) => typeof part?.type === "string" && part.type.startsWith("tool-")) as any[];
  const textPart = parts.find((part: any) => part?.type === "text") as any;
  const usagePart = parts.find((part: any) => part?.type === "SESSION_MESSAGE_PART_TYPE_USAGE") as any;
  const text = typeof textPart?.text === "string" ? textPart.text : "";
  const textChunks = [
    text.slice(0, 54),
    text.slice(54, 128),
    text.slice(128),
  ].filter(Boolean);
  const reasoningText = typeof reasoningPart?.text === "string" ? reasoningPart.text : "";
  const reasoningChunks = [
    reasoningText.slice(0, 70),
    reasoningText.slice(70),
  ].filter(Boolean);

  const stream = new ReadableStream<Uint8Array>({
    async start(controller) {
      const enqueue = async (line: string, wait = 360) => {
        if (signal?.aborted) {
          controller.close();
          return false;
        }
        await delay(wait);
        if (signal?.aborted) {
          controller.close();
          return false;
        }
        controller.enqueue(encoder.encode(line));
        return true;
      };

      if (!(await enqueue(uiStreamLine("f", { messageId: streamingAssistantMessage.id }), 180))) return;
      for (const chunk of reasoningChunks) {
        if (!(await enqueue(uiStreamLine("g", chunk)))) return;
      }
      for (const toolPart of toolParts) {
        if (!(await enqueue(uiStreamLine("9", {
          toolCallId: toolPart.toolCallId,
          toolName: toolPart.toolName,
          args: toolPart.input,
        })))) return;
        if (!(await enqueue(uiStreamLine("a", {
          toolCallId: toolPart.toolCallId,
          result: toolPart.output,
        })))) return;
      }
      for (const chunk of textChunks) {
        if (!(await enqueue(uiStreamLine("0", chunk)))) return;
      }
      if (usagePart?.payloadJson) {
        await enqueue(uiStreamLine("h", JSON.parse(usagePart.payloadJson)), 220);
      }
      controller.close();
    },
  });

  return new Response(stream, {
    status: 200,
    headers: { "Content-Type": "text/plain; charset=utf-8" },
  });
}

function createStreamingGatewayClient() {
  let submitted = false;
  return {
    createSession: async () => ({ sessionId: "storybook-streaming-session" }),
    listSessionMessages: async () => ({
      messages: submitted ? fixedMessages : [],
      hasMore: false,
      state: submitted ? "IDLE" : "RUNNING",
    }),
    markSubmitted: () => {
      submitted = true;
    },
  };
}

function MockStreamingGateway({ children, gateway }: { children: React.ReactNode; gateway: ReturnType<typeof createStreamingGatewayClient> }) {
  globalThis.fetch = async (_input, init) => {
    if (init?.method === "DELETE") {
      return new Response(null, { status: 204 });
    }
    if (init?.method === "POST") {
      gateway.markSubmitted();
      return createStreamingResponse(init?.signal);
    }
    return originalFetch(_input, init);
  };

  useEffect(() => {
    return () => {
      globalThis.fetch = originalFetch;
    };
  }, []);

  return children;
}

function AutoSubmitPrompt() {
  useEffect(() => {
    const timeoutId = window.setTimeout(() => {
      const textarea = document.querySelector<HTMLTextAreaElement>("textarea");
      if (!textarea) return;
      const valueSetter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
      valueSetter?.call(textarea, streamingPrompt);
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
      window.setTimeout(() => {
        textarea.form?.requestSubmit();
      }, 80);
    }, 500);

    return () => window.clearTimeout(timeoutId);
  }, []);

  return null;
}

const meta = {
  title: "Talon Chat/TalonSession",
  component: TalonSession,
  tags: ["autodocs"],
  args: {
    namespace: "support",
    agent: "triage",
    gatewayUrl: "http://localhost:18789",
    gatewayClient,
    sessionId: "storybook-session",
    autoFocus: false,
    placeholder: "Ask Talon about the incident...",
    enabledBuiltInCommands: ["clear"],
  },
  render: (args) => (
    <div style={{ height: "100%", padding: 24, overflow: "hidden" }}>
      <div style={{ height: "min(680px, calc(100vh - 48px))", maxWidth: 480, margin: "0 auto", border: "1px dotted var(--talon-chat-border, rgba(212,212,216,0.7))", background: "var(--talon-chat-surface, #fff)" }}>
        <TalonSession {...args} />
      </div>
    </div>
  ),
} satisfies Meta<TalonSessionProps>;

export default meta;
type Story = StoryObj<typeof meta>;

export const ExistingSession: Story = {};

export const Disabled: Story = {
  args: {
    disabled: true,
    placeholder: "The copilot is temporarily unavailable",
  },
};

export const ImageInputEnabled: Story = {
  args: {
    placeholder: "Ask Talon to inspect an image...",
    onImageUpload: mockImageUpload,
  },
};

export const StreamingResponse: Story = {
  args: {
    sessionId: undefined,
    autoFocus: false,
    placeholder: "Streaming mock response...",
  },
  render: (args) => {
    const streamingGateway = createStreamingGatewayClient();
    return (
      <MockStreamingGateway gateway={streamingGateway}>
        <AutoSubmitPrompt />
        <div style={{ height: "100%", padding: 24, overflow: "hidden" }}>
          <div style={{ height: "min(680px, calc(100vh - 48px))", maxWidth: 480, margin: "0 auto", border: "1px dotted var(--talon-chat-border, rgba(212,212,216,0.7))", background: "var(--talon-chat-surface, #fff)" }}>
            <TalonSession
              {...args}
              gatewayClient={streamingGateway}
              sessionId={undefined}
            />
          </div>
        </div>
      </MockStreamingGateway>
    );
  },
};
