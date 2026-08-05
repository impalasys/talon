import type { SessionTarget } from "./types";
import type { SessionHistoryPage } from "./history";

export type SessionClient = {
  listMessages(target: SessionTarget, options?: { beforeMessageId?: string | null; pageSize?: number; signal?: AbortSignal }): Promise<SessionHistoryPage>;
  create(target: Omit<SessionTarget, "sessionId">): Promise<SessionTarget>;
  clear(target: SessionTarget): Promise<void>;
  stopGeneration(target: SessionTarget, signal?: AbortSignal): Promise<void>;
};

export type SessionClientSource = {
  create?: (target: { ns: string; agent: string }) => Promise<{ sessionId: string }>;
  clear?: (target: SessionTarget) => Promise<unknown>;
  listMessages?: (request: { ns: string; agent: string; sessionId: string; pageSize?: number; beforeMessageId?: string }) => Promise<any>;
  stopGeneration?: (target: SessionTarget, options?: { signal?: AbortSignal }) => Promise<unknown>;
};

export function createSessionClient(source: SessionClientSource, normalizePage: (response: any) => SessionHistoryPage, pageSize = 50): SessionClient {
  return {
    async listMessages(target, options) {
      if (!source.listMessages) throw new Error("Session client does not expose listMessages().");
      return normalizePage(await source.listMessages({
        ...target,
        pageSize: options?.pageSize ?? pageSize,
        beforeMessageId: options?.beforeMessageId || undefined,
      }));
    },
    async create(target) {
      if (!source.create) throw new Error("Session client does not expose create().");
      const result = await source.create(target);
      return { ...target, sessionId: result.sessionId };
    },
    async clear(target) {
      if (!source.clear) throw new Error("Session client does not expose clear().");
      await source.clear(target);
    },
    async stopGeneration(target, signal) {
      if (!source.stopGeneration) throw new Error("Session client does not expose stopGeneration().");
      await source.stopGeneration(target, { signal });
    },
  };
}
