import { useCallback } from "react";
import { fetchResourceFromGateway } from "./resourceLoader";
import type { GatewayClientLike } from "./TalonSessionTypes";
import type { ResourceViewModel } from "../lib/resourceUris";
import { useResourcePane } from "./useResourcePane";

type UseSessionResourcesOptions = {
  agent: string;
  currentSessionId: string | null;
  fetchResource?: (uri: string, signal: AbortSignal) => Promise<ResourceViewModel>;
  gatewayClient: GatewayClientLike;
  sessionId?: string;
};

/** Selects host or gateway loading and owns the built-in resource pane state. */
export function useSessionResources({ agent, currentSessionId, fetchResource, gatewayClient, sessionId }: UseSessionResourcesOptions) {
  const loadResource = useCallback(
    (uri: string, signal: AbortSignal) => fetchResource
      ? fetchResource(uri, signal)
      : fetchResourceFromGateway({ uri, gatewayClient, agent, sessionId: currentSessionId ?? sessionId ?? null, signal }),
    [agent, currentSessionId, fetchResource, gatewayClient, sessionId],
  );
  return useResourcePane(loadResource);
}
