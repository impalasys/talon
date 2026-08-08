import { useCallback } from "react";
import type { SessionActionsOptions } from "./sessionActionTypes";
import type { SessionTarget } from "./types";

type UseSessionTargetResolutionOptions = Pick<SessionActionsOptions,
  "activateTarget" | "agent" | "client" | "currentSessionRef" | "namespace" | "onSessionChange" | "sessionId"
>;

/** Creates a session only when the transcript does not already have an active target. */
export function useSessionTargetResolution({
  activateTarget,
  agent,
  client,
  currentSessionRef,
  namespace,
  onSessionChange,
  sessionId,
}: UseSessionTargetResolutionOptions) {
  const createSession = useCallback(async (): Promise<SessionTarget> => {
    if (!client?.create) throw new Error("TalonSession requires a Talon clientset with sessions.create().");
    const response = await client.create({ ns: namespace, agent });
    return { ns: namespace, agent, sessionId: response.sessionId };
  }, [agent, client, namespace]);

  return useCallback(async (): Promise<SessionTarget> => {
    let session = currentSessionRef.current;
    if (session) return session;
    session = sessionId ? { ns: namespace, agent, sessionId } : await createSession();
    currentSessionRef.current = session;
    activateTarget(session, { hydrate: false });
    if (!sessionId) onSessionChange?.(session.sessionId);
    return session;
  }, [activateTarget, agent, createSession, currentSessionRef, namespace, onSessionChange, sessionId]);
}
