import { useCallback, useEffect, useRef, useState } from "react";
import type { CopilotMessage } from "../../lib/chatTimeline";
import { parsePayloadJson } from "../protocol";
import { objectRefFromValue, objectRefKey } from "../objectRefs";
import type { TalonChatObjectRef } from "../types";

type SessionClient = {
  readToolResultPart?: (request: {
    sessionId: string;
    agent: string;
    ns: string;
    ref: string;
  }) => Promise<any>;
};

type SessionTarget = { sessionId: string; agent: string; ns: string } | null;
export type ToolResultHydrationState = "loading" | { objectKey: string };

type ToolResultPartMatch = {
  part: any;
  partIndex: number;
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

function findObjectPart(parts: unknown, toolCallId: string, wantedPartIndex?: number): ToolResultPartMatch | null {
  if (!Array.isArray(parts)) return null;
  for (const part of parts) {
    if (!part || typeof part !== "object" || !isToolResultPart(part)) continue;
    if (toolCallId && toolCallIdFromPart(part) !== toolCallId) continue;
    const payload = parsePayloadJson(part?.payloadJson ?? part?.payload_json);
    const output = (payload.tool_output ?? payload.toolOutput) as Record<string, unknown> | undefined;
    const contentParts = output?.content_parts ?? output?.contentParts;
    if (!Array.isArray(contentParts)) continue;
    for (let partIndex = 0; partIndex < contentParts.length; partIndex += 1) {
      if (wantedPartIndex !== undefined && wantedPartIndex !== partIndex) continue;
      const contentPart = contentParts[partIndex];
      const object = objectRefFromValue(contentPart);
      const key = objectRefKey(object);
      if (object && key) return { part, partIndex, key, object };
    }
  }
  return null;
}

function findHydratableObjectPart(parts: unknown, toolCallId: string): ToolResultPartMatch | null {
  const match = findObjectPart(parts, toolCallId);
  const content = match?.part.content;
  if (!match || (typeof content === "string" && content.length > 0)) return null;
  const mediaType = (match.object as any).mediaType ?? (match.object as any).media_type ?? "";
  const normalizedType = typeof mediaType === "string" ? mediaType.split(";")[0]!.trim().toLowerCase() : "";
  const isText = normalizedType.startsWith("text/")
    || ["application/json", "application/yaml", "application/x-yaml", "application/toml", "application/xml", "application/javascript", "application/x-javascript"].includes(normalizedType)
    || normalizedType.endsWith("+json")
    || normalizedType.endsWith("+xml");
  return isText ? match : null;
}

function findToolOutputParts(parts: unknown, toolCallId: string): { part: any; values: any[] } | null {
  if (!Array.isArray(parts)) return null;
  for (const part of parts) {
    if (!part || typeof part !== "object" || !isToolResultPart(part)) continue;
    if (toolCallId && toolCallIdFromPart(part) !== toolCallId) continue;
    const payload = parsePayloadJson(part?.payloadJson ?? part?.payload_json);
    const output = (payload.tool_output ?? payload.toolOutput) as Record<string, unknown> | undefined;
    const values = output?.content_parts ?? output?.contentParts;
    if (Array.isArray(values)) return { part, values };
  }
  return null;
}

function textFromOutput(value: any): string | null {
  const output = value?.output ?? value;
  const contentParts = output?.contentParts ?? output?.content_parts;
  if (Array.isArray(contentParts)) {
    const text = contentParts
      .filter((part: any) =>
        (part?.type === "text" && typeof part.text === "string")
        || (part?.content?.case === "text" && typeof part.content.value === "string"),
      )
      .map((part: any) => part.type === "text" ? part.text : part.content.value)
      .join("");
    return text || null;
  }
  return typeof output?.summary === "string" ? output.summary : null;
}

export function useToolResultHydration(sessions: SessionClient | undefined, target: SessionTarget, sessionKey: string | null) {
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
    const result = findToolOutputParts(message.parts, toolCallId);
    if (!result) return fallback;
    const parts = result.values.map((value, partIndex) => {
      if (value?.type === "text" && typeof value.text === "string") return { kind: "text", text: value.text };
      const object = objectRefFromValue(value);
      const key = objectRefKey(object);
      if (!object || !key) return { kind: "attachment", label: "Attachment" };
      const keyForOutput = cacheKey(message.id, toolCallId, key);
      if (Object.prototype.hasOwnProperty.call(outputs, keyForOutput)) {
        return { kind: "text", text: outputs[keyForOutput] };
      }
      const mediaType = object.mediaType ?? object.media_type ?? "";
      const normalizedType = mediaType.split(";")[0]!.trim().toLowerCase();
      const readable = normalizedType.startsWith("text/") || normalizedType === "application/json" || normalizedType.endsWith("+json") || normalizedType.endsWith("+xml");
      return readable
        ? { kind: "external-text", partIndex, objectKey: key }
        : { kind: "attachment", label: object.filename || mediaType || "Attachment" };
    });
    return { kind: "tool-output-parts", parts };
  }, [outputs]);

  const hydrate = useCallback(async (
    message: CopilotMessage,
    toolCallId: string,
    toolRowKey: string,
    _fallback: unknown,
    partIndex?: number,
  ) => {
    const match = partIndex === undefined
      ? findHydratableObjectPart(message.parts, toolCallId)
      : findObjectPart(message.parts, toolCallId, partIndex);
    if (!match || !target || !sessions?.readToolResultPart) return;

    const outputKey = cacheKey(message.id, toolCallId, match.key);
    if (Object.prototype.hasOwnProperty.call(outputs, outputKey) || inFlight.current.has(toolRowKey)) return;
    inFlight.current.add(toolRowKey);
    const currentGeneration = generation.current;
    setState((current) => ({ ...current, [toolRowKey]: "loading" }));
    try {
      const ref = `tr://${encodeURIComponent(toolCallId)}/parts/${match.partIndex}`;
      const response = await sessions.readToolResultPart({
        sessionId: target.sessionId,
        agent: target.agent,
        ns: target.ns,
        ref,
      });
      const output = textFromOutput(response);
      if (output === null) throw new Error("Session service did not return text for this result part");
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
      console.warn("Could not hydrate transcript tool-result part", match.key, error);
      setState((current) => ({ ...current, [toolRowKey]: { objectKey: match.key } }));
    } finally {
      inFlight.current.delete(toolRowKey);
    }
  }, [outputs, sessions, target]);

  return { state, resultFor, hydrate, invalidate };
}
