import { useEffect, useState } from "react";
import { getMessageContent } from "../lib/chatTimeline";
import type { GatewayClientLike } from "./TalonSessionTypes";
import type { SessionTarget } from "./types";

export type PendingSessionMessage = {
  entryId: string;
  content: string;
};

const NEXT_QUEUE = "next";
const REFRESH_INTERVAL_MS = 1_500;

function normalizeQueuedMessages(response: any): PendingSessionMessage[] {
  if (!Array.isArray(response?.entries)) return [];
  return response.entries.flatMap((entry: any, index: number) => {
    const message = entry?.message;
    if (!message) return [];
    const entryId = typeof entry.entryId === "string"
      ? entry.entryId
      : typeof entry.entry_id === "string"
        ? entry.entry_id
        : String(index);
    const content = getMessageContent(message).trim();
    return content ? [{ entryId, content }] : [];
  });
}

/** Polls the durable NEXT queue; these messages are intentionally absent from session history. */
export function useSessionPendingMessages(
  client: GatewayClientLike["sessions"],
  session: SessionTarget | null,
) {
  const [pendingMessages, setPendingMessages] = useState<PendingSessionMessage[]>([]);

  useEffect(() => {
    if (!session || !client.listQueuedMessages) {
      setPendingMessages([]);
      return;
    }
    const controller = new AbortController();
    const refresh = async () => {
      try {
        const response = await client.listQueuedMessages!({
          ns: session.ns,
          agent: session.agent,
          sessionId: session.sessionId,
          queue: NEXT_QUEUE,
        }, { signal: controller.signal });
        if (!controller.signal.aborted) setPendingMessages(normalizeQueuedMessages(response));
      } catch {
        // Queue visibility is supplementary to the transcript. Keep the last
        // successful value while a transient refresh fails.
      }
    };
    void refresh();
    const intervalId = window.setInterval(() => void refresh(), REFRESH_INTERVAL_MS);
    return () => {
      controller.abort();
      window.clearInterval(intervalId);
    };
  }, [client, session]);

  return pendingMessages;
}
