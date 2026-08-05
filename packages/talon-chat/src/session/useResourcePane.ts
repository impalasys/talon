import { useCallback, useRef, useState } from "react";
import type { ResourceViewModel } from "../lib/resourceUris";

export type ResourcePaneLoader = (uri: string, signal: AbortSignal) => Promise<ResourceViewModel>;

export function useResourcePane(fetchResource?: ResourcePaneLoader) {
  const [openResourceUri, setOpenResourceUri] = useState<string | null>(null);
  const [resourcePaneOpen, setResourcePaneOpen] = useState(false);
  const [resourceView, setResourceView] = useState<ResourceViewModel | null>(null);
  const [resourceLoading, setResourceLoading] = useState(false);
  const [resourceError, setResourceError] = useState<Error | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  const open = useCallback(async (uri: string) => {
    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    setOpenResourceUri(uri);
    setResourcePaneOpen(true);
    setResourceLoading(true);
    setResourceError(null);
    try {
      if (fetchResource) setResourceView(await fetchResource(uri, controller.signal));
    } catch (error) {
      if (!controller.signal.aborted) setResourceError(error instanceof Error ? error : new Error(String(error)));
    } finally {
      if (!controller.signal.aborted) setResourceLoading(false);
    }
  }, [fetchResource]);

  const close = useCallback(() => {
    abortRef.current?.abort();
    setResourcePaneOpen(false);
  }, []);

  return { openResourceUri, resourcePaneOpen, resourceView, resourceLoading, resourceError, open, close, abortRef };
}
