import type React from "react";
import type { TalonClient } from "@impalasys/talon-client";

export type GatewayClientLike = {
  sessions: Pick<
    TalonClient["sessions"],
    "create" | "clear" | "compact" | "doctor" | "listMessages" | "submitTurn" | "streamParts" | "stopGeneration"
  > & Partial<Pick<TalonClient["sessions"], "appendMessage" | "updateMessage" | "listQueuedMessages">>;
  cas?: CasServiceClientLike;
  artifacts?: ArtifactServiceClientLike;
  files?: FileServiceClientLike;
};

export type CasServiceClientLike = Pick<TalonClient["cas"], "getObject">;

export type ArtifactServiceClientLike = Pick<
  TalonClient["artifacts"],
  "readArtifact" | "getArtifactMetadata"
> & Partial<Pick<TalonClient["artifacts"], "listArtifacts">>;

export type FileServiceClientLike = Pick<
  TalonClient["files"],
  "readFile" | "getFileMetadata"
>;

export type ResourceUriKind = "artifact" | "file";

export type ParsedResourceUri =
  | {
      kind: "artifact";
      uri: string;
      namespace: string;
      agent: string;
      sessionId: string;
      artifactId: string;
    }
  | {
      kind: "file";
      uri: string;
      namespace: string;
      fileName: string;
    };

export type ResourceViewModel = {
  kind: ResourceUriKind;
  uri: string;
  title: string;
  mediaType: string;
  content?: Uint8Array | string;
  signedUrl?: string;
  /** Immutable CAS/object-store key, if supplied by the gateway. */
  objectKey?: string;
  path?: string;
  sessionId?: string;
  agent?: string;
};

export function parseResourceUri(value: string): ParsedResourceUri | null;
export function isResourceUri(value: string): boolean;
export function linkifyResourceUris(markdown: string): string;
export function toResourceMarkdownHref(uri: string): string;
export function resourceUriFromHref(href: string | null | undefined): string | null;
export function resourceUriShortLabel(uri: string): string;
export const RESOURCE_MARKDOWN_HREF_PREFIX: string;

export type ToolInvocationItem = {
  toolCallId: string;
  toolName: string;
  args: unknown;
  result?: unknown;
};

export type AssistantTimelineItem =
  | { type: "text"; text: string }
  | { type: "reasoning"; text: string }
  | { type: "compaction" }
  | { type: "usage"; usage: UsageSummary }
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
  /** Browser-only presentation content; never persisted or sent to Talon. */
  renderNode?: React.ReactNode;
};

export type TalonBuiltInCommandName = "clear" | "goal" | "compact" | "doctor";

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

export type TalonChatComposerCommandMenuItem = {
  name: string;
  aliases?: string[];
  description?: string;
};

export type TalonChatComposerImageAttachment = {
  id: string;
  filename: string;
  previewUrl: string;
  status?: "queued" | "uploading" | "ready" | "error";
  error?: string;
};

export type TalonChatComposerVariant = "panel" | "compact" | "expanded" | "inline";

export type TalonChatComposerProps = {
  value: string;
  onValueChange: (value: string) => void;
  onSubmit: (value: string) => void;
  placeholder: string;
  variant?: TalonChatComposerVariant;
  autoFocus?: boolean;
  disabled?: boolean;
  rows?: number;
  canSubmit?: boolean;
  isGenerating?: boolean;
  canStop?: boolean;
  onStop?: () => void;
  helperText?: string;
  submitLabel?: string;
  stopLabel?: string;
  textareaMinHeight?: number;
  textareaMaxHeight?: number | string;
  commandMenuItems?: TalonChatComposerCommandMenuItem[];
  startAdornment?: React.ReactNode;
  endAdornment?: React.ReactNode;
  imageAttachments?: TalonChatComposerImageAttachment[];
  imageUploadEnabled?: boolean;
  imageAccept?: string;
  imageButtonLabel?: string;
  onImageFilesSelected?: (files: File[]) => void;
  onRemoveImageAttachment?: (id: string) => void;
  style?: React.CSSProperties;
};

export type TalonSessionPendingImageAttachment = {
  id: string;
  file: File;
  previewUrl: string;
  object?: TalonChatObjectRef;
  status: "queued" | "uploading" | "ready" | "error";
  error?: string;
};

export type TalonSessionSubmitContext = {
  text: string;
  namespace: string;
  agent: string;
  sessionId: string | null;
  imageAttachments: ReadonlyArray<TalonSessionPendingImageAttachment>;
  ensureSession: () => Promise<{ ns: string; agent: string; sessionId: string }>;
  clearInput: () => void;
  refreshSession: () => Promise<void>;
};

export type TalonSessionSubmissionTransformer = (context: TalonSessionSubmitContext) => Promise<{
  message: string;
  displayText?: string;
}> | {
  message: string;
  displayText?: string;
};

export type TalonSessionTurnCompleteContext = {
  namespace: string;
  agent: string;
  sessionId: string;
};

export type TalonSessionMessageDisplayTransformer = (message: CopilotMessage) => CopilotMessage;

export type TalonSessionMessageEditContext = {
  message: CopilotMessage;
  nextContent: string;
  namespace: string;
  agent: string;
  sessionId: string | null;
};

export type TalonSessionCommandTarget = {
  type: "session";
  namespace: string;
  agent: string;
  sessionId: string | null;
};

export type TalonSessionCommand = TalonChatCommand<TalonSessionCommandTarget, CopilotMessage>;

export type TalonChatObjectRef = {
  key: string;
  mediaType?: string;
  media_type?: string;
  sizeBytes?: number | bigint | string;
  size_bytes?: number | bigint | string;
  sha256?: string;
  filename?: string;
  metadata?: Record<string, string>;
};

export type TalonImageUploadContext = {
  file: File;
  namespace: string;
  agent: string;
  sessionId: string;
  signal: AbortSignal;
};

export type TalonImageUploadResult = TalonChatObjectRef | {
  object: TalonChatObjectRef;
  url?: string;
};

export type TalonSessionProps = {
  namespace: string;
  agent: string;
  gatewayClient: GatewayClientLike;
  sessionId?: string;
  /** Labels passed when this component lazily creates a new session. Ignored when sessionId is supplied. */
  sessionCreateLabels?: Record<string, string>;
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
  /**
   * Uploads an image selected in the composer and returns the stored object ref.
   * TalonSession performs client-side type and size checks for UX only; callers
   * must validate file type, size, and content again in this upload handler
   * before storing or processing the file.
   */
  onImageUpload?: (context: TalonImageUploadContext) => Promise<TalonImageUploadResult>;
  objectUrlForRef?: (object: TalonChatObjectRef) => string | undefined;
  maxImageAttachments?: number;
  /**
   * Client-side image size limit in bytes. This improves UX only and must be
   * enforced again by the onImageUpload implementation.
   */
  maxImageBytes?: number;
  /**
   * Client-side accepted image MIME types. This can be bypassed by callers and
   * must be enforced again by the onImageUpload implementation.
   */
  acceptedImageTypes?: string[];
  composerVariant?: TalonChatComposerVariant;
  composerStartAdornment?: React.ReactNode;
  composerEndAdornment?: React.ReactNode;
  onSubmitMessage?: (context: TalonSessionSubmitContext) => Promise<boolean | void> | boolean | void;
  submissionTransformer?: TalonSessionSubmissionTransformer;
  onTurnComplete?: (context: TalonSessionTurnCompleteContext) => Promise<void> | void;
  messageDisplayTransformer?: TalonSessionMessageDisplayTransformer;
  /** Show elapsed-time, reasoning, tool, and usage details for assistant messages. */
  showWorkDetails?: boolean;
  /** Rendered while the agent is processing a turn, instead of elapsed time. */
  loadingIndicator?: React.ReactNode;
  allowMessageEditing?: boolean;
  onMessageEdit?: (context: TalonSessionMessageEditContext) => Promise<boolean | void> | boolean | void;
  enableDebugMessageEditing?: boolean;
  /** Show the current session's Artifact catalog in a collapsible corner card. */
  showSessionArtifacts?: boolean;
  /**
   * Called when an artifact:// or file:// link is clicked.
   * If omitted, the built-in split pane opens when the matching client is available.
   */
  onResourceClick?: (uri: string) => void;
  /**
   * Override content fetch for the built-in resource pane (both kinds).
   */
  fetchResource?: (uri: string, signal: AbortSignal) => Promise<ResourceViewModel>;
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
  channels: Pick<TalonClient["channels"], "listMessages" | "postMessage">;
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
  gatewayClient: ChannelGatewayClientLike;
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
  /**
   * Called when an artifact:// or file:// link is clicked in a channel message.
   * Channels do not open a built-in split pane in phase 1.
   */
  onResourceClick?: (uri: string) => void;
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
  gatewayClient: ChannelGatewayClientLike;
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
export function TalonChatComposer(props: TalonChatComposerProps): React.JSX.Element;
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
