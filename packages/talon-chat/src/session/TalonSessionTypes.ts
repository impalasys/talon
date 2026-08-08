import type React from "react";
import type { TalonClient } from "@impalasys/talon-client";
import type { CopilotMessage } from "../lib/chatTimeline";
import type { TalonBuiltInCommandName, TalonChatCommand } from "../lib/commands";
import type { TalonChatComposerVariant } from "../lib/TalonChatComposer";
import type { ResourceViewModel } from "../lib/resourceUris";
import type { TalonChatObjectRef, TalonSessionHandle } from "./types";

export type { TalonChatObjectRef } from "./types";

/** The gateway capabilities used by a Talon session. */
export type SessionServiceClientLike = {
  sessions: Pick<
    TalonClient["sessions"],
    "create" | "clear" | "listMessages" | "submitTurn" | "streamParts" | "stopGeneration"
  > & Partial<Pick<TalonClient["sessions"], "appendMessage" | "updateMessage">>;
}["sessions"];

export type CasServiceClientLike = Pick<TalonClient["cas"], "getObject">;
export type ArtifactServiceClientLike = Pick<TalonClient["artifacts"], "readArtifact" | "getArtifactMetadata">;
export type FileServiceClientLike = Pick<TalonClient["files"], "readFile" | "getFileMetadata">;

export type GatewayClientLike = {
  sessions: SessionServiceClientLike;
  cas?: CasServiceClientLike;
  artifacts?: ArtifactServiceClientLike;
  files?: FileServiceClientLike;
};

export type TalonSessionCommandTarget = {
  type: "session";
  namespace: string;
  agent: string;
  sessionId: string | null;
};

export type TalonSessionCommand = TalonChatCommand<TalonSessionCommandTarget, CopilotMessage>;

/** Context shared by every attachment uploader. */
export type TalonAttachmentUploadContext = {
  file: File;
  namespace: string;
  agent: string;
  sessionId: string;
  signal: AbortSignal;
};

export type TalonAttachmentUploadResult = TalonChatObjectRef | {
  object: TalonChatObjectRef;
  url?: string;
};

/** Session-local attachment state; inline previews are optional. */
export type TalonSessionPendingAttachment = {
  id: string;
  file: File;
  previewUrl?: string;
  object?: TalonChatObjectRef;
  status: "queued" | "uploading" | "ready" | "error";
  error?: string;
};

/** @deprecated Use TalonAttachmentUploadContext. */
export type TalonImageUploadContext = TalonAttachmentUploadContext;
/** @deprecated Use TalonAttachmentUploadResult. */
export type TalonImageUploadResult = TalonAttachmentUploadResult;
/** @deprecated Use TalonSessionPendingAttachment. */
export type TalonSessionPendingImageAttachment = TalonSessionPendingAttachment;

export type TalonSessionSubmitContext = {
  text: string;
  namespace: string;
  agent: string;
  sessionId: string | null;
  attachments: ReadonlyArray<TalonSessionPendingAttachment>;
  /** @deprecated Use attachments. */
  imageAttachments: ReadonlyArray<TalonSessionPendingAttachment>;
  ensureSession: () => Promise<TalonSessionHandle>;
  clearInput: () => void;
  refreshSession: () => Promise<void>;
};

export type TalonSessionMessageEditContext = {
  message: CopilotMessage;
  nextContent: string;
  namespace: string;
  agent: string;
  sessionId: string | null;
};

export type TalonSessionProps = {
  namespace: string;
  agent: string;
  gatewayClient: GatewayClientLike;
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
  /** Upload boundary for composer attachments; handlers must validate file content. */
  onAttachmentUpload?: (context: TalonAttachmentUploadContext) => Promise<TalonAttachmentUploadResult>;
  /** @deprecated Use onAttachmentUpload. */
  onImageUpload?: (context: TalonImageUploadContext) => Promise<TalonImageUploadResult>;
  objectUrlForRef?: (object: TalonChatObjectRef) => string | undefined;
  maxAttachments?: number;
  maxAttachmentBytes?: number;
  acceptedAttachmentTypes?: string[];
  /** @deprecated Use maxAttachments. */
  maxImageAttachments?: number;
  /** Client-side only; enforce the size limit again in the upload handler. */
  /** @deprecated Use maxAttachmentBytes. */
  maxImageBytes?: number;
  /** Client-side only; enforce accepted types again in the upload handler. */
  /** @deprecated Use acceptedAttachmentTypes. */
  acceptedImageTypes?: string[];
  composerVariant?: TalonChatComposerVariant;
  composerStartAdornment?: React.ReactNode;
  composerEndAdornment?: React.ReactNode;
  onSubmitMessage?: (context: TalonSessionSubmitContext) => Promise<boolean | void> | boolean | void;
  allowMessageEditing?: boolean;
  onMessageEdit?: (context: TalonSessionMessageEditContext) => Promise<boolean | void> | boolean | void;
  enableDebugMessageEditing?: boolean;
  /** Override artifact:// and file:// interaction instead of opening the built-in pane. */
  onResourceClick?: (uri: string) => void;
  /** Override resource fetching for the built-in pane. */
  fetchResource?: (uri: string, signal: AbortSignal) => Promise<ResourceViewModel>;
};

export type TalonCopilotProps = TalonSessionProps;
