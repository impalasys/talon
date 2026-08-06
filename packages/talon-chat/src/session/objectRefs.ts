import type { TalonChatObjectRef, TalonImageUploadResult } from "../TalonSession";

export function objectRefMediaType(object: TalonChatObjectRef | undefined): string {
  return object?.mediaType || object?.media_type || "";
}

export function objectRefSizeBytes(object: TalonChatObjectRef): number {
  const value = object.sizeBytes ?? object.size_bytes ?? 0;
  if (typeof value === "bigint") return Number(value);
  if (typeof value === "string") {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : 0;
  }
  return Number.isFinite(value) ? value : 0;
}

export function normalizeObjectRef(object: TalonChatObjectRef): TalonChatObjectRef {
  return {
    key: object.key,
    mediaType: object.mediaType ?? object.media_type ?? "",
    sizeBytes: objectRefSizeBytes(object),
    sha256: object.sha256 ?? "",
    filename: object.filename ?? "",
    metadata: object.metadata ?? {},
  };
}

export function normalizeImageUploadResult(result: TalonImageUploadResult): TalonChatObjectRef {
  return "object" in result ? result.object : result;
}

export function objectRefFromValue(value: unknown): TalonChatObjectRef | undefined {
  if (!value || typeof value !== "object") return undefined;

  const candidate = value as Record<string, unknown>;
  for (const key of ["object", "objectRef", "object_ref"]) {
    const nested = candidate[key];
    if (nested && typeof nested === "object" && typeof (nested as TalonChatObjectRef).key === "string") {
      return nested as TalonChatObjectRef;
    }
  }

  const toolOutput = candidate.tool_output ?? candidate.toolOutput;
  const output = toolOutput && typeof toolOutput === "object"
    ? toolOutput as Record<string, unknown>
    : candidate;
  const contentParts = output.content_parts ?? output.contentParts;
  if (Array.isArray(contentParts)) {
    for (const part of contentParts) {
      const nested = objectRefFromValue(part);
      if (nested) return nested;
    }
  }
  return undefined;
}

export function normalizeObjectRefForJson(object: TalonChatObjectRef): TalonChatObjectRef {
  return normalizeObjectRef(object);
}

function parsePayload(value: unknown): Record<string, unknown> {
  if (typeof value !== "string" || value.length === 0) return {};
  try {
    const parsed = JSON.parse(value);
    return parsed && typeof parsed === "object" ? parsed as Record<string, unknown> : {};
  } catch {
    return {};
  }
}

export function objectRefFromPart(
  part: unknown,
  parsePayloadJson: (value: unknown) => Record<string, unknown> = parsePayload,
): TalonChatObjectRef | undefined {
  const candidate = part as Record<string, unknown> | null;
  if (!candidate || typeof candidate !== "object") return undefined;
  const object = candidate.object ?? candidate.objectRef ?? candidate.object_ref;
  if (object && typeof object === "object") return object as TalonChatObjectRef;
  return objectRefFromValue(parsePayloadJson(candidate.payloadJson ?? candidate.payload_json));
}

export function objectRefKey(object: TalonChatObjectRef | undefined): string {
  return typeof object?.key === "string" ? object.key : "";
}

export function objectRefContentEncoding(object: TalonChatObjectRef | undefined): string {
  return object?.contentEncoding
    ?? object?.content_encoding
    ?? object?.metadata?.content_encoding
    ?? object?.metadata?.contentEncoding
    ?? "";
}
