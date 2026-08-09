import { useCallback, useState } from "react";

/** Owns expanded assistant-detail and tool-result rows in one transcript. */
export function useTranscriptExpansionState() {
  const [expandedThinkingMessages, setExpandedThinkingMessages] = useState<Record<string, boolean>>({});
  const [expandedToolItems, setExpandedToolItems] = useState<Record<string, boolean>>({});

  const toggleThinkingMessage = useCallback((messageId: string) => {
    setExpandedThinkingMessages((current) => ({ ...current, [messageId]: !current[messageId] }));
  }, []);
  const toggleToolItem = useCallback((toolKey: string) => {
    setExpandedToolItems((current) => ({ ...current, [toolKey]: !current[toolKey] }));
  }, []);
  const reset = useCallback(() => {
    setExpandedThinkingMessages({});
    setExpandedToolItems({});
  }, []);

  return { expandedThinkingMessages, expandedToolItems, reset, toggleThinkingMessage, toggleToolItem };
}
