import { useCallback, useEffect, useRef, useState } from "react";
import type { ResourceViewModel } from "../../lib/resourceUris";

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
    setResourceView(null);
    try {
      if (fetchResource) {
        const view = await fetchResource(uri, controller.signal);
        if (controller.signal.aborted || abortRef.current !== controller) return;
        setResourceView(view);
      }
    } catch (error) {
      if (!controller.signal.aborted && abortRef.current === controller) {
        setResourceError(error instanceof Error ? error : new Error(String(error)));
      }
    } finally {
      if (!controller.signal.aborted && abortRef.current === controller) setResourceLoading(false);
    }
  }, [fetchResource]);

  const close = useCallback(() => {
    abortRef.current?.abort();
    setResourcePaneOpen(false);
  }, []);

  const reset = useCallback(() => {
    abortRef.current?.abort();
    abortRef.current = null;
    setOpenResourceUri(null);
    setResourcePaneOpen(false);
    setResourceView(null);
    setResourceLoading(false);
    setResourceError(null);
  }, []);

  const completeClose = useCallback(() => {
    setOpenResourceUri(null);
    setResourceView(null);
    setResourceLoading(false);
    setResourceError(null);
  }, []);

  useEffect(() => () => abortRef.current?.abort(), []);

  return {
    openResourceUri,
    resourcePaneOpen,
    resourceView,
    resourceLoading,
    resourceError,
    open,
    close,
    reset,
    completeClose,
    abortRef,
  };
}
