import type React from "react";

export type GatewayClientLike = {
  createSession(request: { ns: string; agent: string }): Promise<{ sessionId: string }>;
  clearSession?(request: { ns: string; agent: string; sessionId: string }): Promise<any>;
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
  parts?: unknown;
  reasoningContent?: string;
  timeline?: AssistantTimelineItem[];
  usage?: UsageSummary;
  toolInvocations?: ToolInvocationItem[];
};

export type TalonBuiltInCommandName = "clear";

export type TalonChatCommandContext<TTarget, TMessage> = {
  name: string;
  input: string;
  args: string;
  argv: string[];
  target: TTarget;
  messages: TMessage[];
  clear?: () => void | Promise<void>;
};

export type TalonChatCommand<TTarget = unknown, TMessage = unknown> = {
  name: string;
  aliases?: string[];
  description?: string;
  run: (context: TalonChatCommandContext<TTarget, TMessage>) => void | Promise<void>;
};

export type TalonSessionCommandTarget = {
  type: "session";
  namespace: string;
  agent: string;
  sessionId: string | null;
};

export type TalonSessionCommand = TalonChatCommand<TalonSessionCommandTarget, CopilotMessage>;

export type TalonSessionProps = {
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
  historyPageSize?: number;
  historyMessageLimit?: number;
  historyStepLimit?: number;
  commands?: TalonSessionCommand[];
  enabledBuiltInCommands?: TalonBuiltInCommandName[];
};

export type TalonCopilotProps = TalonSessionProps;

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

export type TalonChannelCommandTarget = {
  type: "channel";
  namespace: string;
  channel: string;
  status: string;
};

export type TalonChannelCommand = TalonChatCommand<TalonChannelCommandTarget, ChannelMessage>;

export type ChannelGatewayClientLike = {
  listChannelMessages(request: {
    ns: string;
    channel: string;
    limit?: number;
    pageSize?: number;
    beforeMessageId?: string;
  }): Promise<any>;
  postChannelMessage(request: {
    ns: string;
    channel: string;
    authorKind: string;
    author: string;
    content: string;
    subscriptionNames?: string[];
    labels?: Record<string, string>;
  }): Promise<any>;
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
  gatewayClient?: ChannelGatewayClientLike;
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
  commands?: TalonChannelCommand[];
};

export type UseTalonChannelMessagesOptions = {
  namespace: string;
  channel: string | {
    name?: string;
    ns?: string;
    title?: string;
    status?: string;
    metadata?: Record<string, string>;
    labels?: Record<string, string>;
  } | null | undefined;
  gatewayUrl: string;
  authToken?: string | null;
  gatewayClient?: ChannelGatewayClientLike;
  disabled?: boolean;
  messageLimit?: number;
  refreshIntervalMs?: number | false;
};

export type UseTalonChannelMessagesResult = {
  channelName: string;
  status: string;
  messages: ChannelMessage[];
  isLoading: boolean;
  isLoadingOlderMessages: boolean;
  hasMoreMessages: boolean;
  error: string | null;
  refresh: (options?: { silent?: boolean; replace?: boolean }) => Promise<void>;
  loadOlderMessages: () => Promise<void>;
  postMessage: (options: { author: string; authorKind: string; content: string }) => Promise<void>;
};

export function TalonSession(props: TalonSessionProps): React.JSX.Element;
export const TalonCopilot: typeof TalonSession;
export function TalonChannel(props: TalonChannelProps): React.JSX.Element;
export function useTalonChannelMessages(
  options: UseTalonChannelMessagesOptions,
): UseTalonChannelMessagesResult;
export function buildGatewayHeaders(
  authToken?: string | null,
): { Authorization: string } | undefined;
export function normalizeGatewayUrl(url: string): string;
export function applyGatewayAuthorizationHeader(
  headerTarget: { set(name: string, value: string): void },
  authToken?: string | null,
): void;
