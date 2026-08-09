import { useCallback, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import type { TalonClient } from "@impalasys/talon-client";
import { getMessageContent, type CopilotMessage } from "../lib/chatTimeline";
import {
  isLocalMessageId,
  normalizeRawSessionMessage,
} from "./history";
import {
  editableMessageContent,
  messageWithEditedContent,
  replaceMessageTextPart,
} from "./messageEditing";
import { messagePartsForSessionUpdate } from "./protocol";
import type { SessionTarget } from "./types";

const LABEL_CONNECTOR_DELIVERY_STATUS = "talon.impalasys.com/connector-delivery-status";
const LABEL_CONNECTOR_DELIVERY_ERROR = "talon.impalasys.com/connector-delivery-error";

export type SessionMessageEditContext = {
  message: CopilotMessage;
  nextContent: string;
  namespace: string;
  agent: string;
  sessionId: string | null;
};

type SessionMessageUpdateClient = Partial<Pick<TalonClient["sessions"], "updateMessage">>;

type UseSessionMessageEditingOptions = {
  agent: string;
  client: SessionMessageUpdateClient;
  currentSessionRef: MutableRefObject<SessionTarget | null>;
  enableDebugMessageEditing: boolean;
  fallbackSessionId: string | undefined;
  messagesRef: MutableRefObject<CopilotMessage[]>;
  namespace: string;
  onMessageEdit?: (context: SessionMessageEditContext) => Promise<boolean | void> | boolean | void;
  setError: Dispatch<SetStateAction<Error | null>>;
  setMessages: Dispatch<SetStateAction<CopilotMessage[]>>;
};

type SessionMessageUpdaterOptions = Pick<
  UseSessionMessageEditingOptions,
  "agent" | "client" | "currentSessionRef" | "fallbackSessionId" | "messagesRef" | "namespace" | "setMessages"
>;

function errorFromUnknown(error: unknown) {
  return error instanceof Error ? error : new Error(String(error));
}

function shouldPersistSessionEdit(message: CopilotMessage, enabled: boolean) {
  return enabled &&
    (message.role === "user" || message.role === "assistant") &&
    !isLocalMessageId(message.id);
}

function useSessionMessageUpdater({
  agent,
  client,
  currentSessionRef,
  fallbackSessionId,
  messagesRef,
  namespace,
  setMessages,
}: SessionMessageUpdaterOptions) {
  return useCallback(
    async (message: CopilotMessage, parts: unknown[], labels: Record<string, string>) => {
      const session = currentSessionRef.current ?? (fallbackSessionId
        ? { ns: namespace, agent, sessionId: fallbackSessionId }
        : null);
      if (!session) throw new Error("No active session to update.");
      if (!client.updateMessage) {
        throw new Error("Gateway client does not support sessions.updateMessage().");
      }
      const response = await client.updateMessage({
        ns: session.ns,
        agent: session.agent,
        sessionId: session.sessionId,
        messageId: message.id,
        parts,
        labels,
      });
      const updated = response?.message
        ? { ...message, ...normalizeRawSessionMessage(response.message) }
        : { ...message, parts, labels, content: getMessageContent({ ...message, parts }) };
      setMessages((current) => {
        const nextMessages = current.map((item) => item.id === message.id ? updated : item);
        messagesRef.current = nextMessages;
        return nextMessages;
      });
      return updated;
    },
    [agent, client, currentSessionRef, fallbackSessionId, messagesRef, namespace, setMessages],
  );
}

/** Owns local editing state and the two message-update workflows used by a transcript row. */
export function useSessionMessageEditing({
  agent,
  client,
  currentSessionRef,
  enableDebugMessageEditing,
  fallbackSessionId,
  messagesRef,
  namespace,
  onMessageEdit,
  setError,
  setMessages,
}: UseSessionMessageEditingOptions) {
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [editingMessageValue, setEditingMessageValue] = useState("");
  const [reviewActionMessageId, setReviewActionMessageId] = useState<string | null>(null);
  const updateSessionMessage = useSessionMessageUpdater({
    agent,
    client,
    currentSessionRef,
    fallbackSessionId,
    messagesRef,
    namespace,
    setMessages,
  });

  const startEditingMessage = useCallback((message: CopilotMessage) => {
    setEditingMessageId(message.id);
    setEditingMessageValue(editableMessageContent(message));
  }, []);

  const cancelEditingMessage = useCallback(() => {
    setEditingMessageId(null);
    setEditingMessageValue("");
  }, []);

  const applyLocalEdit = useCallback((message: CopilotMessage, nextContent: string) => {
    setMessages((current) => {
      const nextMessages = current.map((item) => item.id === message.id
        ? messageWithEditedContent(item, nextContent)
        : item);
      messagesRef.current = nextMessages;
      return nextMessages;
    });
  }, [messagesRef, setMessages]);

  const saveEditingMessage = useCallback(async (message: CopilotMessage) => {
    const nextContent = editingMessageValue.trim();
    if (!nextContent) return;

    setError(null);
    try {
      if (shouldPersistSessionEdit(message, enableDebugMessageEditing)) {
        setReviewActionMessageId(message.id);
        await updateSessionMessage(message, replaceMessageTextPart(message, nextContent), { ...(message.labels ?? {}) });
        cancelEditingMessage();
        return;
      }
      const handled = await onMessageEdit?.({
        message,
        nextContent,
        namespace,
        agent,
        sessionId: currentSessionRef.current?.sessionId ?? fallbackSessionId ?? null,
      });
      if (handled === false) return;

      applyLocalEdit(message, nextContent);
      cancelEditingMessage();
    } catch (error) {
      setError(errorFromUnknown(error));
    } finally {
      setReviewActionMessageId(null);
    }
  }, [agent, applyLocalEdit, cancelEditingMessage, currentSessionRef, editingMessageValue, enableDebugMessageEditing, fallbackSessionId, namespace, onMessageEdit, setError, updateSessionMessage]);

  const updateConnectorDeliveryStatus = useCallback(
    async (message: CopilotMessage, status: string) => {
      const labels = {
        ...(message.labels ?? {}),
        [LABEL_CONNECTOR_DELIVERY_STATUS]: status,
      };
      delete labels[LABEL_CONNECTOR_DELIVERY_ERROR];
      setError(null);
      setReviewActionMessageId(message.id);
      try {
        await updateSessionMessage(message, messagePartsForSessionUpdate(message), labels);
      } catch (error) {
        setError(errorFromUnknown(error));
      } finally {
        setReviewActionMessageId(null);
      }
    },
    [setError, updateSessionMessage],
  );

  return {
    editingMessageId,
    editingMessageValue,
    reviewActionMessageId,
    setEditingMessageValue,
    startEditingMessage,
    cancelEditingMessage,
    saveEditingMessage,
    updateConnectorDeliveryStatus,
  };
}
