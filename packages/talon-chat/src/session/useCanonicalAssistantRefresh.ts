import { useCallback } from "react";
import type { SessionActionsOptions } from "./sessionActionTypes";
import { assistantSignature, sameSession } from "./sessionSubmission";
import type { SessionTarget } from "./types";

type UseCanonicalAssistantRefreshOptions = Pick<SessionActionsOptions,
  "currentSessionRef" | "refreshNewestSessionPage" | "refreshRuntime"
>;

/** Waits for canonical history when a stream ends before emitting an assistant event. */
export function useCanonicalAssistantRefresh({
  currentSessionRef,
  refreshNewestSessionPage,
  refreshRuntime,
}: UseCanonicalAssistantRefreshOptions) {
  return useCallback(async (session: SessionTarget, baselineSignature: string, signal?: AbortSignal) => {
    for (let attempt = 0; attempt < 40; attempt += 1) {
      if (signal?.aborted || !sameSession(currentSessionRef.current, session)) return false;
      const state = await refreshRuntime(session, signal);
      if (signal?.aborted || !sameSession(currentSessionRef.current, session) || !state) return false;
      if (assistantSignature(state.messages) && assistantSignature(state.messages) !== baselineSignature) {
        await refreshNewestSessionPage(session, signal);
        return true;
      }
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    return false;
  }, [currentSessionRef, refreshNewestSessionPage, refreshRuntime]);
}
