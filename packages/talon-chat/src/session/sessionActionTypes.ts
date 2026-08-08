import type { TalonClient } from "@impalasys/talon-client";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import type { CopilotMessage } from "../lib/chatTimeline";
import type { StreamEventItem } from "../lib/uiStream";
import type { SessionHistoryPage } from "./history";
import type {
  TalonSessionCommand,
  TalonSessionPendingImageAttachment,
  TalonSessionSubmitContext,
} from "./TalonSessionTypes";
import type { SessionTarget } from "./types";

export type SessionActionsClient = Pick<TalonClient["sessions"], "create" | "submitTurn">;
export type RefreshSession = (target: SessionTarget, signal?: AbortSignal) => Promise<SessionHistoryPage | null>;

export type SessionActionsOptions = {
  client: SessionActionsClient | undefined;
  namespace: string;
  agent: string;
  sessionId?: string;
  disabled: boolean;
  isSessionLive: boolean;
  enabledGoalCommand: boolean;
  commands: TalonSessionCommand[];
  onSessionChange?: (sessionId: string) => void;
  onSubmitMessage?: (context: TalonSessionSubmitContext) => Promise<boolean | void> | boolean | void;
  currentSessionRef: MutableRefObject<SessionTarget | null>;
  messagesRef: MutableRefObject<CopilotMessage[]>;
  imageAttachmentsRef: MutableRefObject<TalonSessionPendingImageAttachment[]>;
  submissionAbortControllerRef: MutableRefObject<AbortController | null>;
  submittedPreviewUrlsRef: MutableRefObject<string[]>;
  resolvedHistoryPageSize: number;
  setInput: Dispatch<SetStateAction<string>>;
  setImageAttachments: Dispatch<SetStateAction<TalonSessionPendingImageAttachment[]>>;
  setMessages: Dispatch<SetStateAction<CopilotMessage[]>>;
  setStreamEvents: Dispatch<SetStateAction<StreamEventItem[]>>;
  setError: (error: Error | null) => void;
  setIsLoading: (value: boolean) => void;
  setIsResuming: (value: boolean) => void;
  setLoadingStartedAt: (value: string | number | null) => void;
  setLoadingNow: (value: number) => void;
  activateTarget: (target: SessionTarget, options?: { hydrate?: boolean }) => void;
  uploadQueuedImages: (target: SessionTarget, signal: AbortSignal) => Promise<TalonSessionPendingImageAttachment[]>;
  clearSession: () => Promise<void>;
  cancelResume: () => void;
  startResume: (target: SessionTarget) => void;
  isStoppingRef: MutableRefObject<boolean>;
  markAutoScrollPinned: () => void;
  refreshRuntime: RefreshSession;
  refreshNewestSessionPage: RefreshSession;
};
