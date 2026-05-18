import type React from "react";

export type GatewayClientLike = {
  createSession(request: { ns: string; agent: string }): Promise<{ sessionId: string }>;
  getSession(request: { ns: string; agent: string; sessionId: string }): Promise<any>;
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
};

export function TalonCopilot(props: TalonCopilotProps): React.JSX.Element;
export function buildGatewayHeaders(
  authToken?: string | null,
): { Authorization: string } | undefined;
export function normalizeGatewayUrl(url: string): string;
export function applyGatewayAuthorizationHeader(
  headerTarget: { set(name: string, value: string): void },
  authToken?: string | null,
): void;
