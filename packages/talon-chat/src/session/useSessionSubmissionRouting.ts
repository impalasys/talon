import { useCallback } from "react";
import { findTalonChatCommand, parseTalonChatCommandInput } from "../lib/commands";
import type { TalonSessionPendingImageAttachment } from "./TalonSessionTypes";
import type { SessionActionsOptions } from "./sessionActionTypes";
import type { SessionTarget } from "./types";

type UseSessionSubmissionRoutingOptions = Pick<SessionActionsOptions,
  "agent" | "clearSession" | "commands" | "currentSessionRef" | "messagesRef" | "namespace" |
  "onSubmitMessage" | "refreshNewestSessionPage" | "sessionId" | "setError" | "setInput" | "setStreamEvents"
> & { ensureSession: () => Promise<SessionTarget> };

export type PreparedSubmission = {
  text: string;
  parsedCommand: ReturnType<typeof parseTalonChatCommandInput>;
  error: Error | null;
};

export function prepareSubmission(text: string, enabledGoalCommand: boolean): PreparedSubmission {
  const parsedCommand = parseTalonChatCommandInput(text);
  if (parsedCommand?.name !== "goal" || !enabledGoalCommand) return { text, parsedCommand, error: null };
  const goalText = parsedCommand.args?.trim() ?? "";
  if (!goalText) return { text, parsedCommand: null, error: new Error("Usage: /goal <objective and success criteria>") };
  return {
    text: ["Create or update a Talon Goal for this session.", "", "Use the goal tools directly. Track this objective until completion:", goalText].join("\n"),
    parsedCommand: null,
    error: null,
  };
}

/** Handles host overrides and slash commands before a normal backend turn starts. */
export function useSessionSubmissionRouting({
  agent,
  clearSession,
  commands,
  currentSessionRef,
  ensureSession,
  messagesRef,
  namespace,
  onSubmitMessage,
  refreshNewestSessionPage,
  sessionId,
  setError,
  setInput,
  setStreamEvents,
}: UseSessionSubmissionRoutingOptions) {
  const runHostSubmission = useCallback(async (text: string, attachments: TalonSessionPendingImageAttachment[]) => {
    if (!onSubmitMessage) return false;
    setError(null);
    try {
      return Boolean(await onSubmitMessage({
        text,
        namespace,
        agent,
        sessionId: currentSessionRef.current?.sessionId ?? sessionId ?? null,
        attachments,
        imageAttachments: attachments,
        ensureSession,
        clearInput: () => setInput(""),
        refreshSession: async () => { await refreshNewestSessionPage(await ensureSession()); },
      }));
    } catch (error) {
      setError(error instanceof Error ? error : new Error(String(error)));
      return true;
    }
  }, [agent, currentSessionRef, ensureSession, namespace, onSubmitMessage, refreshNewestSessionPage, sessionId, setError, setInput]);

  const runSessionCommand = useCallback(async (
    text: string,
    parsedCommand: ReturnType<typeof parseTalonChatCommandInput>,
    hasImages: boolean,
  ) => {
    const command = findTalonChatCommand(commands, parsedCommand);
    if (!command || !parsedCommand || hasImages) return false;
    setInput("");
    setError(null);
    setStreamEvents([]);
    try {
      await command.run({
        name: parsedCommand.name,
        input: text,
        args: parsedCommand.args,
        argv: parsedCommand.argv,
        target: { type: "session", namespace, agent, sessionId: currentSessionRef.current?.sessionId ?? sessionId ?? null },
        messages: messagesRef.current,
        clear: clearSession,
      });
    } catch (error) {
      setError(error instanceof Error ? error : new Error(String(error)));
    }
    return true;
  }, [agent, clearSession, commands, currentSessionRef, messagesRef, namespace, sessionId, setError, setInput, setStreamEvents]);

  return { runHostSubmission, runSessionCommand };
}
