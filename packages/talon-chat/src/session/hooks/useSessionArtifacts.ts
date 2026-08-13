import { useCallback, useEffect, useRef, useState } from "react";
import type { GatewayClientLike } from "../TalonSessionTypes";
import {
  mergeSessionArtifacts,
  normalizeSessionArtifact,
  type SessionArtifact,
  type SessionArtifactTarget,
} from "../artifacts";

const PAGE_SIZE = 50;

type UseSessionArtifactsOptions = {
  enabled: boolean;
  gatewayClient: GatewayClientLike;
  target: SessionArtifactTarget | null;
};

function targetKey(target: SessionArtifactTarget | null) {
  return target ? `${target.ns}\u0000${target.agent}\u0000${target.sessionId}` : "";
}

/** Loads a session-scoped Artifact catalog without allowing old requests to leak into a new session. */
export function useSessionArtifacts({ enabled, gatewayClient, target }: UseSessionArtifactsOptions) {
  const listArtifacts = gatewayClient.artifacts?.listArtifacts;
  const available = Boolean(enabled && target && listArtifacts);
  const scopeKey = targetKey(target);
  const requestRef = useRef<AbortController | null>(null);
  const scopeRef = useRef(scopeKey);
  // Update during render so a response from the prior session cannot briefly
  // populate the rail before the scope-change effect gets to abort it.
  scopeRef.current = scopeKey;
  const [artifacts, setArtifacts] = useState<SessionArtifact[]>([]);
  const [nextPageToken, setNextPageToken] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const isLoadingRef = useRef(false);

  const load = useCallback(async (reset: boolean, pageToken = ""): Promise<SessionArtifact[] | null> => {
    if (!available || !target || !listArtifacts) return null;
    if (!reset && (!pageToken || isLoadingRef.current)) return null;
    requestRef.current?.abort();
    const controller = new AbortController();
    requestRef.current = controller;
    const requestScope = targetKey(target);
    isLoadingRef.current = true;
    setIsLoading(true);
    setError(null);
    try {
      const response = await (listArtifacts as any)(
        {
          namespace: target.ns,
          agent: target.agent,
          sessionId: target.sessionId,
          limit: PAGE_SIZE,
          pageToken,
        },
        { signal: controller.signal },
      );
      if (controller.signal.aborted || requestRef.current !== controller || scopeRef.current !== requestScope) return;
      const fetched = (Array.isArray(response?.artifacts) ? response.artifacts : [])
        .map(normalizeSessionArtifact)
        .filter((artifact): artifact is SessionArtifact => artifact !== null);
      setArtifacts((current) => mergeSessionArtifacts(reset ? [] : current, fetched));
      setNextPageToken(typeof (response?.nextPageToken ?? response?.next_page_token) === "string"
        ? response.nextPageToken ?? response.next_page_token
        : "");
      return fetched;
    } catch (reason) {
      if (!controller.signal.aborted && requestRef.current === controller && scopeRef.current === requestScope) {
        setError(reason instanceof Error ? reason : new Error(String(reason)));
      }
      return null;
    } finally {
      if (!controller.signal.aborted && requestRef.current === controller && scopeRef.current === requestScope) {
        isLoadingRef.current = false;
        setIsLoading(false);
      }
    }
  }, [available, listArtifacts, target]);

  const refresh = useCallback(() => load(true), [load]);
  const loadMore = useCallback(() => load(false, nextPageToken), [load, nextPageToken]);

  useEffect(() => {
    scopeRef.current = scopeKey;
    requestRef.current?.abort();
    requestRef.current = null;
    isLoadingRef.current = false;
    setArtifacts([]);
    setNextPageToken("");
    setIsLoading(false);
    setError(null);
    if (available) void load(true);
  }, [available, load, scopeKey]);

  useEffect(() => () => requestRef.current?.abort(), []);

  return { artifacts, available, error, hasMore: Boolean(nextPageToken), isLoading, loadMore, refresh };
}
