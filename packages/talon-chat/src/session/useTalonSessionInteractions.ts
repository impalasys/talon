import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import type { CopilotMessage } from "../lib/chatTimeline";
import { useSessionMessageEditing } from "./useSessionMessageEditing";
import { useSessionResourceClick } from "./useSessionResourceClick";
import { useSessionResources } from "./useSessionResources";
import type { TalonSessionProps } from "./TalonSessionTypes";
import type { SessionTarget } from "./types";

type Options = Pick<TalonSessionProps, "agent" | "fetchResource" | "gatewayClient" | "onMessageEdit" | "onResourceClick" | "sessionId"> & {
  currentSession: SessionTarget | null;
  currentSessionRef: MutableRefObject<SessionTarget | null>;
  enableDebugMessageEditing: boolean;
  messagesRef: MutableRefObject<CopilotMessage[]>;
  namespace: string;
  setError: (error: Error | null) => void;
  setMessages: Dispatch<SetStateAction<CopilotMessage[]>>;
};

/** Owns resource-pane and message-edit controls that do not affect generation state. */
export function useTalonSessionInteractions({
  agent, currentSession, currentSessionRef, enableDebugMessageEditing, fetchResource, gatewayClient,
  messagesRef, namespace, onMessageEdit, onResourceClick, sessionId, setError, setMessages,
}: Options) {
  const resources = useSessionResources({ agent, currentSessionId: currentSession?.sessionId ?? null, fetchResource, gatewayClient, sessionId });
  const editing = useSessionMessageEditing({
    agent, client: gatewayClient.sessions, currentSessionRef, enableDebugMessageEditing, fallbackSessionId: sessionId,
    messagesRef, namespace, onMessageEdit, setError, setMessages,
  });
  const handleResourceClick = useSessionResourceClick({
    canFetchArtifact: Boolean(fetchResource) || Boolean(gatewayClient.artifacts?.readArtifact),
    canFetchFile: Boolean(fetchResource) || Boolean(gatewayClient.files?.readFile),
    closeResourcePane: resources.close, onResourceClick, openResource: resources.open,
    openResourceUri: resources.openResourceUri, resourcePaneOpen: resources.resourcePaneOpen,
  });
  return { editing, handleResourceClick, resources };
}
