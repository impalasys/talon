const MAX_INLINE_PREVIEW_BYTES = 3 * 1024 * 1024;

function isInlinePreviewMediaType(mediaType: string): boolean {
  const base = mediaType.split(";", 1)[0]?.trim().toLowerCase() || "";
  return base.startsWith("text/") || base === "application/json";
}

function byteLength(value: unknown): number | null {
  if (typeof value === "bigint") return value <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(value) : null;
  if (typeof value === "number" && Number.isFinite(value) && value >= 0) return value;
  if (typeof value === "string" && /^\d+$/.test(value)) {
    const parsed = Number(value);
    return Number.isSafeInteger(parsed) ? parsed : null;
  }
  return null;
}

/** Fetch a signed object URL only when its bytes can be rendered inline. */
export async function loadSignedInlineContent({
  content,
  mediaType,
  signedUrl,
  sizeBytes,
  signal,
}: {
  content: Uint8Array | undefined;
  mediaType: string;
  signedUrl: string | undefined;
  sizeBytes: unknown;
  signal: AbortSignal;
}): Promise<Uint8Array | undefined> {
  const knownSize = byteLength(sizeBytes);
  if (
    !signedUrl ||
    (content && content.byteLength > 0) ||
    !isInlinePreviewMediaType(mediaType) ||
    (knownSize != null && knownSize > MAX_INLINE_PREVIEW_BYTES)
  ) {
    return content;
  }
  const response = await fetch(signedUrl, { signal, credentials: "omit" });
  if (!response.ok) throw new Error(`Could not download resource content (HTTP ${response.status}).`);
  return new Uint8Array(await response.arrayBuffer());
}
