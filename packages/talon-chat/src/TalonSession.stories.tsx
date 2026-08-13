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
  sessions: {
    create: async () => ({ sessionId: "storybook-session" }),
    clear: async () => ({}),
    listMessages: async () => ({
      messages: fixedMessages,
      hasMore: false,
      state: "IDLE",
    }),
    submitTurn: async function* () {},
    streamParts: async function* () {},
    stopGeneration: async () => ({}),
  },
};

const mockImageUpload: TalonSessionProps["onImageUpload"] = async ({ file, namespace, agent, sessionId }) => ({
  key: `${namespace}/${agent}/${sessionId}/uploads/${file.name}`,
  mediaType: file.type || "image/png",
  sizeBytes: file.size,
  filename: file.name,
});

const streamingPrompt = "Summarize the latest incident notes and identify the next owner.";
const streamingAssistantMessage = fixedMessages[1];

function delay(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function* createStreamingEvents(signal?: AbortSignal | null) {
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

  const emit = async (event: any, wait = 360) => {
    if (signal?.aborted) return false;
    await delay(wait);
    return !signal?.aborted && event;
  };

  const first = await emit({ kind: 1, messageId: streamingAssistantMessage.id }, 180);
  if (!first) return;
  yield first;

  for (const chunk of reasoningChunks) {
    const event = await emit({
      kind: 1,
      messageId: streamingAssistantMessage.id,
      part: { partType: 2, content: chunk },
    });
    if (!event) return;
    yield event;
  }
  for (const toolPart of toolParts) {
    const call = await emit({
      kind: 1,
      messageId: streamingAssistantMessage.id,
      part: {
        id: toolPart.toolCallId,
        partType: 3,
        name: toolPart.toolName,
        payloadJson: JSON.stringify({ tool_call_id: toolPart.toolCallId, input: toolPart.input }),
      },
    });
    if (!call) return;
    yield call;

    const result = await emit({
      kind: 1,
      messageId: streamingAssistantMessage.id,
      part: {
        id: toolPart.toolCallId,
        partType: 4,
        payloadJson: JSON.stringify({ tool_call_id: toolPart.toolCallId, output: toolPart.output }),
      },
    });
    if (!result) return;
    yield result;
  }
  for (const chunk of textChunks) {
    const event = await emit({
      kind: 1,
      messageId: streamingAssistantMessage.id,
      part: { partType: 1, content: chunk },
    });
    if (!event) return;
    yield event;
  }
  if (usagePart?.payloadJson) {
    const usage = await emit({
      kind: 1,
      messageId: streamingAssistantMessage.id,
      part: { partType: 5, payloadJson: usagePart.payloadJson },
    }, 220);
    if (!usage) return;
    yield usage;
  }
  yield { kind: 2, messageId: streamingAssistantMessage.id };
}

function createStreamingGatewayClient() {
  let submitted = false;
  const client: GatewayClientLike = {
    sessions: {
      create: async () => ({ sessionId: "storybook-streaming-session" }),
      clear: async () => {
        submitted = false;
        return {};
      },
      listMessages: async () => ({
        messages: submitted ? fixedMessages : [],
        hasMore: false,
        state: submitted ? "IDLE" : "RUNNING",
      }),
      submitTurn: async function* (_request, options) {
        submitted = true;
        yield* createStreamingEvents(options?.signal);
      },
      streamParts: async function* (_request, options) {
        yield* createStreamingEvents(options?.signal);
      },
      stopGeneration: async () => ({}),
    },
  };
  return client;
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

const waitForResourcePaneOpen: NonNullable<Story["play"]> = async ({ canvasElement }) => {
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    if (canvasElement.querySelector('[data-testid="talon-resource-pane"][data-open="true"]')) {
      return;
    }
    await new Promise((resolve) => window.setTimeout(resolve, 50));
  }
  throw new Error("Timed out waiting for resource pane to open");
};

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
      <>
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
      </>
    );
  },
};

const encoder = new TextEncoder();

const ARTIFACT_MD_URI = "artifact://Tenant:acme:Ops/writer/storybook-session/final-draft";
const ARTIFACT_JSON_URI = "artifact://Tenant:acme:Ops/writer/storybook-session/metrics-json";
const ARTIFACT_TEXT_URI = "artifact://Tenant:acme:Ops/writer/storybook-session/plain-notes";
const ARTIFACT_DENIED_URI = "artifact://Tenant:acme:Ops/writer/storybook-session/secret";
const ARTIFACT_SLOW_URI = "artifact://Tenant:acme:Ops/writer/storybook-session/slow-draft";
const FILE_MD_URI = "file://Tenant:acme:Ops/memory-brand-guidelines";
const FILE_JSON_URI = "file://Tenant:acme:Ops/config-snapshot";
const FILE_IMAGE_URI = "file://Tenant:acme:Ops/diagram-png";
const FILE_BINARY_URI = "file://Tenant:acme:Ops/export-bin";

// 1x1 green PNG
const TINY_PNG_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
function tinyPngBytes() {
  const binary = atob(TINY_PNG_BASE64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

const resourceCatalogMessages = [
  {
    id: "resource-user-1",
    role: "ROLE_USER",
    parts: [{ type: "text", text: "Show me the draft, guidelines, and other sample resources." }],
    createdAt: "2026-06-05T16:20:00.000Z",
  },
  {
    id: "resource-assistant-1",
    role: "ROLE_ASSISTANT",
    parts: [
      {
        type: "text",
        text: [
          "### Markdown resources",
          "",
          `- Artifact draft: ${ARTIFACT_MD_URI}`,
          `- Promoted file: ${FILE_MD_URI}`,
          `- Labeled link: [open draft](${ARTIFACT_MD_URI})`,
          "",
          "### Other media types",
          "",
          `- JSON artifact: ${ARTIFACT_JSON_URI}`,
          `- Plain text: ${ARTIFACT_TEXT_URI}`,
          `- JSON file: ${FILE_JSON_URI}`,
          `- Image file: ${FILE_IMAGE_URI}`,
          `- Binary download: ${FILE_BINARY_URI}`,
          "",
          "### Errors / latency",
          "",
          `- Access denied: ${ARTIFACT_DENIED_URI}`,
          `- Slow load (1.2s): ${ARTIFACT_SLOW_URI}`,
          "",
          "Bare URIs and markdown links both open the split pane. Click the same URI again to close.",
        ].join("\n"),
      },
    ],
    createdAt: "2026-06-05T16:20:08.000Z",
  },
];

function createSessionClient(messages: typeof resourceCatalogMessages): GatewayClientLike["sessions"] {
  return {
    create: async () => ({ sessionId: "storybook-session" }),
    clear: async () => ({}),
    listMessages: async () => ({
      messages,
      hasMore: false,
      state: "IDLE",
    }),
    submitTurn: async function* () {},
    streamParts: async function* () {},
    stopGeneration: async () => ({}),
  };
}

const resourceGatewayClient: GatewayClientLike = {
  sessions: createSessionClient(resourceCatalogMessages),
  artifacts: {
    listArtifacts: async ({ pageToken }) => {
      const artifacts = [
        { id: "final-draft", title: "Final draft", mediaType: "text/markdown", objectRef: { sizeBytes: 1_436 }, createdAt: 1_780_750_000_000_000 },
        { id: "metrics-json", title: "Metrics snapshot", mediaType: "application/json", objectRef: { sizeBytes: 84 }, createdAt: 1_780_740_000_000_000 },
        { id: "plain-notes", title: "Plain notes", mediaType: "text/plain", objectRef: { sizeBytes: 42 }, createdAt: 1_780_730_000_000_000 },
      ];
      return pageToken ? { artifacts: [], nextPageToken: "" } : { artifacts, nextPageToken: "more" };
    },
    readArtifact: async ({ artifactUri }) => {
      if (artifactUri === ARTIFACT_DENIED_URI) {
        throw new Error("PermissionDenied: artifact access denied");
      }
      if (artifactUri === ARTIFACT_SLOW_URI) {
        await delay(1200);
        return {
          artifact: {
            id: "slow-draft",
            title: "Slow draft",
            mediaType: "text/markdown",
          },
          content: encoder.encode("# Slow draft\n\nLoaded after a short delay for the loading state.\n"),
          signedUrl: "",
        };
      }
      if (artifactUri === ARTIFACT_JSON_URI) {
        return {
          artifact: {
            id: "metrics-json",
            title: "Metrics snapshot",
            mediaType: "application/json",
          },
          content: encoder.encode(JSON.stringify({ p99_ms: 42, error_rate: 0.001, region: "us-west" }, null, 2)),
          signedUrl: "",
        };
      }
      if (artifactUri === ARTIFACT_TEXT_URI) {
        return {
          artifact: {
            id: "plain-notes",
            title: "Plain notes",
            mediaType: "text/plain",
          },
          content: encoder.encode("line 1\nline 2\nno markdown rendering here\n"),
          signedUrl: "",
        };
      }
      return {
        artifact: {
          id: "final-draft",
          title: "Final draft",
          mediaType: "text/markdown",
          sessionId: "storybook-session",
          createdByAgent: "writer",
        },
        content: encoder.encode(
          `# Final draft\n\nThis mock artifact was opened from \`${artifactUri}\`.\n\n- Item one\n- Item two\n\nNested link: ${FILE_MD_URI}\n`,
        ),
        signedUrl: "",
      };
    },
    getArtifactMetadata: async () => ({
      artifact: {
        id: "final-draft",
        title: "Final draft",
        mediaType: "text/markdown",
      },
    }),
  },
  files: {
    readFile: async ({ file }) => {
      const uri = file?.uri ?? "";
      if (uri === FILE_JSON_URI) {
        return {
          file: {
            metadata: { name: "config-snapshot", namespace: "Tenant:acme:Ops" },
            spec: { path: "/exports/config.json", mediaType: "application/json" },
          },
          content: encoder.encode(JSON.stringify({ featureFlags: { resourcePane: true }, version: 3 }, null, 2)),
          signedUrl: "",
        };
      }
      if (uri === FILE_IMAGE_URI) {
        return {
          file: {
            metadata: { name: "diagram-png", namespace: "Tenant:acme:Ops" },
            spec: { path: "/assets/diagram.png", mediaType: "image/png" },
          },
          content: tinyPngBytes(),
          signedUrl: "",
        };
      }
      if (uri === FILE_BINARY_URI) {
        return {
          file: {
            metadata: { name: "export-bin", namespace: "Tenant:acme:Ops" },
            spec: { path: "/exports/blob.bin", mediaType: "application/octet-stream" },
          },
          content: new Uint8Array([0, 1, 2, 3, 4, 5]),
          signedUrl: "https://example.com/mock-download/export.bin",
        };
      }
      return {
        file: {
          metadata: { name: "memory-brand-guidelines", namespace: "Tenant:acme:Ops" },
          spec: {
            path: "/memory/brand-guidelines.md",
            mediaType: "text/markdown",
          },
        },
        content: encoder.encode(
          `# Brand guidelines\n\nUse plain language.\n\nOpened from \`${uri}\`.\n`,
        ),
        signedUrl: "",
      };
    },
    getFileMetadata: async () => ({
      file: {
        metadata: { name: "memory-brand-guidelines" },
        spec: { path: "/memory/brand-guidelines.md", mediaType: "text/markdown" },
      },
    }),
  },
};

function ResourceSessionFrame(props: { children: React.ReactNode; maxWidth?: number }) {
  return (
    <div style={{ height: "100%", padding: 24, overflow: "hidden" }}>
      <div
        style={{
          height: "min(720px, calc(100vh - 48px))",
          maxWidth: props.maxWidth ?? 960,
          margin: "0 auto",
          border: "1px dotted var(--talon-chat-border, rgba(212,212,216,0.7))",
          background: "var(--talon-chat-surface, #fff)",
        }}
      >
        {props.children}
      </div>
    </div>
  );
}

/** Click bare + labeled artifact:// and file:// URIs; pane opens beside chat. */
export const ResourceUris: Story = {
  name: "Resource URIs (click to open pane)",
  args: {
    gatewayClient: resourceGatewayClient,
    sessionId: "storybook-session",
    placeholder: "Click a resource URI in the assistant message...",
  },
  render: (args) => (
    <ResourceSessionFrame>
      <TalonSession {...args} />
    </ResourceSessionFrame>
  ),
};

/** Session-scoped Artifacts are discoverable without a URI in the transcript. */
export const SessionArtifactCatalog: Story = {
  name: "Session artifacts · corner card",
  args: {
    gatewayClient: resourceGatewayClient,
    sessionId: "storybook-session",
    showSessionArtifacts: true,
    placeholder: "Browse artifacts beside this session...",
  },
  render: (args) => (
    <ResourceSessionFrame maxWidth={960}>
      <TalonSession {...args} />
    </ResourceSessionFrame>
  ),
};

function AutoOpenResourceLink({ uri }: { uri: string }) {
  useEffect(() => {
    const selector = `a[data-resource-uri="${CSS.escape(uri)}"]`;
    let cancelled = false;
    const deadline = Date.now() + 5000;
    const tick = () => {
      if (cancelled) return;
      const link = document.querySelector<HTMLAnchorElement>(selector);
      if (link) {
        link.click();
        return;
      }
      if (Date.now() < deadline) window.setTimeout(tick, 50);
    };
    tick();
    return () => {
      cancelled = true;
    };
  }, [uri]);
  return null;
}

/** Auto-opens a markdown artifact so the split pane is visible without clicking. */
export const ResourcePaneMarkdownArtifact: Story = {
  name: "Resource pane · markdown artifact",
  args: {
    gatewayClient: resourceGatewayClient,
    sessionId: "storybook-session",
    placeholder: "Resource pane preview...",
  },
  render: (args) => (
    <ResourceSessionFrame>
      <AutoOpenResourceLink uri={ARTIFACT_MD_URI} />
      <TalonSession {...args} />
    </ResourceSessionFrame>
  ),
  play: waitForResourcePaneOpen,
};

/** Auto-opens a markdown file URI. */
export const ResourcePaneMarkdownFile: Story = {
  name: "Resource pane · markdown file",
  args: {
    gatewayClient: resourceGatewayClient,
    sessionId: "storybook-session",
    placeholder: "Resource pane preview...",
  },
  render: (args) => (
    <ResourceSessionFrame>
      <AutoOpenResourceLink uri={FILE_MD_URI} />
      <TalonSession {...args} />
    </ResourceSessionFrame>
  ),
  play: waitForResourcePaneOpen,
};

/** Auto-opens JSON content rendered as monospace pre. */
export const ResourcePaneJson: Story = {
  name: "Resource pane · JSON",
  args: {
    gatewayClient: resourceGatewayClient,
    sessionId: "storybook-session",
  },
  render: (args) => (
    <ResourceSessionFrame>
      <AutoOpenResourceLink uri={ARTIFACT_JSON_URI} />
      <TalonSession {...args} />
    </ResourceSessionFrame>
  ),
  play: waitForResourcePaneOpen,
};

/** Auto-opens image/* content. */
export const ResourcePaneImage: Story = {
  name: "Resource pane · image",
  args: {
    gatewayClient: resourceGatewayClient,
    sessionId: "storybook-session",
  },
  render: (args) => (
    <ResourceSessionFrame>
      <AutoOpenResourceLink uri={FILE_IMAGE_URI} />
      <TalonSession {...args} />
    </ResourceSessionFrame>
  ),
  play: waitForResourcePaneOpen,
};

/** Auto-opens binary resource with download affordance. */
export const ResourcePaneBinaryDownload: Story = {
  name: "Resource pane · binary download",
  args: {
    gatewayClient: resourceGatewayClient,
    sessionId: "storybook-session",
  },
  render: (args) => (
    <ResourceSessionFrame>
      <AutoOpenResourceLink uri={FILE_BINARY_URI} />
      <TalonSession {...args} />
    </ResourceSessionFrame>
  ),
  play: waitForResourcePaneOpen,
};

/** Access denied error state in the pane. */
export const ResourcePaneAccessDenied: Story = {
  name: "Resource pane · access denied",
  args: {
    gatewayClient: resourceGatewayClient,
    sessionId: "storybook-session",
  },
  render: (args) => (
    <ResourceSessionFrame>
      <AutoOpenResourceLink uri={ARTIFACT_DENIED_URI} />
      <TalonSession {...args} />
    </ResourceSessionFrame>
  ),
  play: waitForResourcePaneOpen,
};

/** Slow fetch shows the loading state, then content. */
export const ResourcePaneLoading: Story = {
  name: "Resource pane · loading",
  args: {
    gatewayClient: resourceGatewayClient,
    sessionId: "storybook-session",
  },
  render: (args) => (
    <ResourceSessionFrame>
      <AutoOpenResourceLink uri={ARTIFACT_SLOW_URI} />
      <TalonSession {...args} />
    </ResourceSessionFrame>
  ),
  play: waitForResourcePaneOpen,
};

/** Host owns open/view via onResourceClick (no built-in pane). */
export const ResourceUrisHostCallback: Story = {
  name: "Resource URIs · host onResourceClick",
  args: {
    gatewayClient: resourceGatewayClient,
    sessionId: "storybook-session",
    placeholder: "Clicks call onResourceClick only...",
    onResourceClick: (uri: string) => {
      // Storybook actions panel alternative for local preview
      window.alert(`onResourceClick: ${uri}`);
    },
  },
  render: (args) => (
    <ResourceSessionFrame maxWidth={560}>
      <TalonSession {...args} />
    </ResourceSessionFrame>
  ),
};

/** Styled links only — no artifacts/files client and no callback. */
export const ResourceUrisNoClient: Story = {
  name: "Resource URIs · styled only (no client)",
  args: {
    gatewayClient: {
      sessions: createSessionClient(resourceCatalogMessages),
    },
    sessionId: "storybook-session",
    placeholder: "Links are styled but pane cannot open...",
  },
  render: (args) => (
    <ResourceSessionFrame maxWidth={560}>
      <TalonSession {...args} />
    </ResourceSessionFrame>
  ),
};
