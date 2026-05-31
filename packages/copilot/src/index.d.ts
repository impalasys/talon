import type React from "react";

export type GatewayClientLike = {
  createSession(request: { ns: string; agent: string }): Promise<{ sessionId: string }>;
  listSessionMessages?(request: {
    ns: string;
    agent: string;
    sessionId: string;
    pageSize: number;
    beforeMessageId?: string;
  }): Promise<any>;
  getSession(request: { ns: string; agent: string; sessionId: string; messageLimit?: number; stepLimit?: number }): Promise<any>;
};

export type ToolInvocationItem = {
  toolCallId: string;
  toolName: string;
  args: unknown;
  result?: unknown;
};

export type AssistantTimelineItem =
  | { type: "text"; text: string }
  | {
      type: "tool";
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

export type CopilotMessage = {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  createdAt?: string | number | bigint;
  parts?: Array<Record<string, unknown>>;
  reasoningContent?: string;
  timeline?: AssistantTimelineItem[];
  usage?: UsageSummary;
  toolInvocations?: ToolInvocationItem[];
};

export type TalonCopilotProps = {
  namespace: string;
  agent: string;
  gatewayUrl: string;
  authToken?: string | null;
  gatewayClient?: GatewayClientLike;
  sessionId?: string;
  onSessionChange?: (sessionId: string) => void;
  className?: string;
  style?: React.CSSProperties;
  placeholder?: string;
  autoFocus?: boolean;
  disabled?: boolean;
  talonIcon?: React.ReactNode;
  timestampLocale?: Intl.LocalesArgument;
  formatTimestamp?: (message: CopilotMessage) => string;
  historyPageSize?: number;
  historyMessageLimit?: number;
  historyStepLimit?: number;
};

export type ChannelMessage = {
  id?: string;
  ns?: string;
  channel?: string;
  authorKind?: string;
  author_kind?: string;
  author?: string;
  content?: string;
  createdAt?: string | number | bigint;
  created_at?: string | number | bigint;
  sourceAgent?: string;
  source_agent?: string;
  sourceSessionId?: string;
  source_session_id?: string;
};

export type TalonChannelProps = {
  namespace: string;
  channel: string | {
    name?: string;
    ns?: string;
    title?: string;
    status?: string;
    metadata?: Record<string, string>;
    labels?: Record<string, string>;
  };
  gatewayUrl: string;
  authToken?: string | null;
  className?: string;
  style?: React.CSSProperties;
  disabled?: boolean;
  disableUserInput?: boolean;
  author?: string;
  authorKind?: string;
  messageLimit?: number;
  refreshIntervalMs?: number | false;
  timestampLocale?: Intl.LocalesArgument;
  formatTimestamp?: (message: ChannelMessage) => string;
  renderMessageActions?: (message: ChannelMessage) => React.ReactNode;
};

export function TalonCopilot(props: TalonCopilotProps): React.JSX.Element;
export function TalonChannel(props: TalonChannelProps): React.JSX.Element;
export function buildGatewayHeaders(
  authToken?: string | null,
): { Authorization: string } | undefined;
export function normalizeGatewayUrl(url: string): string;
export function applyGatewayAuthorizationHeader(
  headerTarget: { set(name: string, value: string): void },
  authToken?: string | null,
): void;
