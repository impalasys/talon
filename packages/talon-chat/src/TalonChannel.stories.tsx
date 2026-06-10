import React, { useEffect } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { TalonChannel, type TalonChannelProps } from "./TalonChannel";

const channelMessages = [
  {
    id: "018f4b50-8cc0-7000-8000-000000000101",
    authorKind: "agent",
    author: "triage",
    content: "I found a spike in failed ingestion jobs starting at 09:08.",
    createdAt: "2026-06-05T16:08:15.000Z",
    sourceAgent: "triage",
    sourceSessionId: "session-101",
  },
  {
    id: "018f4b50-a234-7000-8000-000000000102",
    authorKind: "user",
    author: "sightline",
    content: "Can you verify whether the rollback guardrail fired?",
    createdAt: "2026-06-05T16:09:22.000Z",
  },
  {
    id: "018f4b52-4444-7000-8000-000000000103",
    authorKind: "agent",
    author: "release-watch",
    content: "Rollback guardrail fired successfully. Queue depth is back under the alert threshold.",
    createdAt: "2026-06-05T16:12:45.000Z",
    sourceAgent: "release-watch",
    sourceSessionId: "session-202",
  },
];

const originalFetch = globalThis.fetch;

const mockedFetch: typeof fetch = async (_input, init) => {
  if (init?.method === "POST") {
    return new Response(JSON.stringify({ ok: true }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }
  return new Response(JSON.stringify({ messages: channelMessages, hasMore: false }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
};

function MockChannelGateway({ children }: { children: React.ReactNode }) {
  globalThis.fetch = mockedFetch;

  useEffect(() => {
    return () => {
      globalThis.fetch = originalFetch;
    };
  }, []);

  return children;
}

const meta = {
  title: "Talon Chat/TalonChannel",
  component: TalonChannel,
  decorators: [
    (Story) => (
      <MockChannelGateway>
        <Story />
      </MockChannelGateway>
    ),
  ],
  tags: ["autodocs"],
  args: {
    namespace: "support",
    channel: { name: "incident-room", status: "open" },
    gatewayUrl: "http://localhost:18789",
    refreshIntervalMs: false,
    formatTimestamp: () => "Jun 5, 2026, 9:12 AM",
    enabledBuiltInCommands: ["clear"],
  },
  render: (args) => (
    <div style={{ height: "100%", padding: 24, overflow: "hidden" }}>
      <div style={{ height: "min(560px, calc(100vh - 48px))", maxWidth: 480, margin: "0 auto", border: "1px solid var(--talon-chat-border, rgba(148,163,184,0.32))", background: "var(--talon-chat-surface, #fff)" }}>
        <TalonChannel {...args} />
      </div>
    </div>
  ),
} satisfies Meta<TalonChannelProps>;

export default meta;
type Story = StoryObj<typeof meta>;

export const OpenChannel: Story = {};

export const ReadOnly: Story = {
  args: {
    disableUserInput: true,
  },
};
