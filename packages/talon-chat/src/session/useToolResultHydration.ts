import { useCallback, useEffect, useRef, useState } from "react";
import { decompress as decompressZstd } from "fzstd";
import type { CopilotMessage } from "../lib/chatTimeline";
import { parsePayloadJson } from "./protocol";
import {
  objectRefContentEncoding,
  objectRefFromPart,
  objectRefFromValue,
  objectRefKey,
} from "./objectRefs";
import type { TalonChatObjectRef } from "./types";

export type ToolResultHydrationState = "loading" | { objectKey: string };

type CasClient = {
  getObject(request: { key: string }): Promise<any>;
};

type ToolResultPartMatch = {
  part: any;
  key: string;
  object: TalonChatObjectRef;
};

function isToolResultPart(part: any) {
  const type = part?.type ?? part?.partType ?? part?.part_type;
  return type === 4 || type === "SESSION_MESSAGE_PART_TYPE_TOOL_RESULT";
}

function toolCallIdFromPart(part: any): string {
  if (typeof part?.toolCallId === "string") return part.toolCallId;
  if (typeof part?.tool_call_id === "string") return part.tool_call_id;
  const payload = parsePayloadJson(part?.payloadJson ?? part?.payload_json);
  const payloadToolCallId = payload.tool_call_id ?? payload.toolCallId;
  if (typeof payloadToolCallId === "string") return payloadToolCallId;
  return typeof part?.id === "string" ? part.id : "";
}

function cacheKey(messageId: string, toolCallId: string, objectKey: string): string {
  return `${messageId}\u0000${toolCallId}\u0000${objectKey}`;
}

function findObjectPart(parts: unknown, toolCallId: string): ToolResultPartMatch | null {
  if (!Array.isArray(parts)) return null;
  for (const part of parts) {
    if (!part || typeof part !== "object" || !isToolResultPart(part)) continue;
    if (toolCallId && toolCallIdFromPart(part) !== toolCallId) continue;
    const object = objectRefFromPart(part);
    const key = objectRefKey(object);
    if (object && key) return { part, key, object };
  }
  return null;
}

function findHydratableObjectPart(parts: unknown, toolCallId: string): ToolResultPartMatch | null {
  const match = findObjectPart(parts, toolCallId);
  return match && !(typeof match.part.content === "string" && match.part.content.length > 0)
    ? match
    : null;
}

function replaceObjectInOutput(part: unknown, fallback: unknown, objectKey: string, hydratedOutput: string): unknown {
  const payload = parsePayloadJson((part as any)?.payloadJson ?? (part as any)?.payload_json);
  const toolOutput = payload.tool_output ?? payload.toolOutput;
  const contentParts = toolOutput && typeof toolOutput === "object"
    ? (toolOutput as Record<string, unknown>).content_parts ?? (toolOutput as Record<string, unknown>).contentParts
    : undefined;
  if (!Array.isArray(contentParts)) return hydratedOutput;

  let replaced = false;
  const output = contentParts.map((contentPart) => {
    if (!contentPart || typeof contentPart !== "object") return "";
    const value = contentPart as { type?: unknown; text?: unknown };
    if (value.type === "text" && typeof value.text === "string") return value.text;
    if (objectRefFromValue(contentPart)?.key === objectKey) {
      replaced = true;
      return hydratedOutput;
    }
    return "";
  }).join("");
  return replaced ? output : fallback;
}

async function decompress(data: Uint8Array, encoding: string): Promise<Uint8Array> {
  if (typeof DecompressionStream === "undefined") {
    throw new Error(`${encoding} CAS object requires DecompressionStream support`);
  }
  const buffer = new ArrayBuffer(data.byteLength);
  new Uint8Array(buffer).set(data);
  const stream = new Blob([buffer]).stream().pipeThrough(new DecompressionStream(encoding as any));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

async function decodeObject(response: any, fallbackObject: TalonChatObjectRef): Promise<Uint8Array> {
  const signedUrl = typeof response?.signedUrl === "string"
    ? response.signedUrl
    : typeof response?.signed_url === "string" ? response.signed_url : "";
  const bytes = signedUrl
    ? await (async () => {
      const fetched = await fetch(signedUrl);
      if (!fetched.ok) throw new Error(`Failed to fetch CAS object: HTTP ${fetched.status}`);
      return new Uint8Array(await fetched.arrayBuffer());
    })()
    : response.data ?? new Uint8Array();
  const encoding = response?.contentEncoding
    ?? response?.content_encoding
    ?? response?.metadata?.content_encoding
    ?? response?.metadata?.contentEncoding
    ?? objectRefContentEncoding(fallbackObject);
  if (typeof encoding !== "string") return bytes;
  if (encoding.toLowerCase() === "gzip") return decompress(bytes, "gzip");
  if (encoding.toLowerCase() === "zstd") {
    if (typeof DecompressionStream !== "undefined") {
      try {
        return await decompress(bytes, "zstd");
      } catch (error) {
        if (!(error instanceof TypeError)) throw error;
      }
    }
    return decompressZstd(bytes);
  }
  return bytes;
}

export function useToolResultHydration(cas: CasClient | undefined, sessionKey: string | null) {
  const [state, setState] = useState<Record<string, ToolResultHydrationState>>({});
  const [outputs, setOutputs] = useState<Record<string, string>>({});
  const inFlight = useRef(new Set<string>());
  const generation = useRef(0);

  const invalidate = useCallback(() => {
    generation.current += 1;
    inFlight.current.clear();
    setState({});
    setOutputs({});
  }, []);

  useEffect(() => {
    invalidate();
  }, [invalidate, sessionKey]);

  const resultFor = useCallback((message: CopilotMessage, toolCallId: string, fallback: unknown): unknown => {
    const partMatch = findObjectPart(message.parts, toolCallId);
    const object = partMatch?.object ?? objectRefFromValue(fallback);
    const key = objectRefKey(object);
    if (!key) return fallback;
    const keyForOutput = cacheKey(message.id, toolCallId, key);
    return Object.prototype.hasOwnProperty.call(outputs, keyForOutput)
      ? replaceObjectInOutput(partMatch?.part, fallback, key, outputs[keyForOutput]!)
      : fallback;
  }, [outputs]);

  const hydrate = useCallback(async (
    message: CopilotMessage,
    toolCallId: string,
    toolRowKey: string,
    fallback: unknown,
  ) => {
    const partMatch = findHydratableObjectPart(message.parts, toolCallId);
    const fallbackObject = partMatch ? undefined : objectRefFromValue(fallback);
    const match = partMatch ?? (fallbackObject
      ? { part: undefined, key: fallbackObject.key, object: fallbackObject }
      : null);
    if (!match || !cas?.getObject) return;

    const outputKey = cacheKey(message.id, toolCallId, match.key);
    if (Object.prototype.hasOwnProperty.call(outputs, outputKey) || inFlight.current.has(toolRowKey)) return;
    inFlight.current.add(toolRowKey);
    const currentGeneration = generation.current;
    setState((current) => ({ ...current, [toolRowKey]: "loading" }));
    try {
      const response = await cas.getObject({ key: match.key });
      const output = new TextDecoder().decode(await decodeObject(response, match.object));
      if (generation.current !== currentGeneration) return;
      setOutputs((current) => ({ ...current, [outputKey]: output }));
      setState((current) => {
        if (!(toolRowKey in current)) return current;
        const next = { ...current };
        delete next[toolRowKey];
        return next;
      });
    } catch (error) {
      if (generation.current !== currentGeneration) return;
      console.warn("Could not hydrate CAS tool-result object", match.key, error);
      setState((current) => ({ ...current, [toolRowKey]: { objectKey: match.key } }));
    } finally {
      inFlight.current.delete(toolRowKey);
    }
  }, [cas, outputs]);

  return { state, resultFor, hydrate, invalidate };
}
