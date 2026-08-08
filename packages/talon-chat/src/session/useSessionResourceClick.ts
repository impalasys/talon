import { useCallback, useRef } from "react";
import { parseResourceUri } from "../lib/resourceUris";

type UseSessionResourceClickOptions = {
  canFetchArtifact: boolean;
  canFetchFile: boolean;
  closeResourcePane: () => void;
  onResourceClick?: (uri: string) => void;
  openResource: (uri: string) => Promise<void>;
  openResourceUri: string | null;
  resourcePaneOpen: boolean;
};

/** Routes artifact and file links to a host override or the built-in resource pane. */
export function useSessionResourceClick({
  canFetchArtifact,
  canFetchFile,
  closeResourcePane,
  onResourceClick,
  openResource,
  openResourceUri,
  resourcePaneOpen,
}: UseSessionResourceClickOptions) {
  const missingResourceClientWarnedRef = useRef(false);

  return useCallback((uri: string) => {
    if (onResourceClick) {
      onResourceClick(uri);
      return;
    }

    const parsed = parseResourceUri(uri);
    if (!parsed) return;

    if (openResourceUri === parsed.uri && resourcePaneOpen) {
      closeResourcePane();
      return;
    }

    const canOpen =
      (parsed.kind === "artifact" && canFetchArtifact) ||
      (parsed.kind === "file" && canFetchFile);
    if (!canOpen) {
      if (!missingResourceClientWarnedRef.current && typeof console !== "undefined") {
        missingResourceClientWarnedRef.current = true;
        console.warn(
          "[talon-chat] Resource link clicked but no artifacts/files client (or fetchResource) is available.",
        );
      }
      return;
    }

    void openResource(parsed.uri);
  }, [canFetchArtifact, canFetchFile, closeResourcePane, onResourceClick, openResource, openResourceUri, resourcePaneOpen]);
}
