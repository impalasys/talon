export type SessionArtifact = {
  id: string;
  title: string;
  mediaType: string;
  /** Immutable CAS/object-store key for detecting a revised artifact body. */
  objectKey: string | undefined;
  sizeBytes: number | bigint | string | undefined;
  createdAt: number | bigint | string | undefined;
};

export type SessionArtifactTarget = {
  ns: string;
  agent: string;
  sessionId: string;
};

function valueOf(source: Record<string, unknown>, camelCase: string, snakeCase: string) {
  return source[camelCase] ?? source[snakeCase];
}

export function normalizeSessionArtifact(value: unknown): SessionArtifact | null {
  if (!value || typeof value !== "object") return null;
  const source = value as Record<string, unknown>;
  const id = valueOf(source, "id", "id");
  if (typeof id !== "string" || !id) return null;
  const objectRef = valueOf(source, "objectRef", "object_ref");
  const object = objectRef && typeof objectRef === "object" ? objectRef as Record<string, unknown> : {};
  const title = valueOf(source, "title", "title");
  const mediaType = valueOf(source, "mediaType", "media_type");
  const objectKey = valueOf(object, "key", "key");
  const sizeBytes = valueOf(object, "sizeBytes", "size_bytes");
  const createdAt = valueOf(source, "createdAt", "created_at");
  return {
    id,
    title: typeof title === "string" ? title : "",
    mediaType: typeof mediaType === "string" ? mediaType : "",
    objectKey: typeof objectKey === "string" && objectKey ? objectKey : undefined,
    sizeBytes: typeof sizeBytes === "number" || typeof sizeBytes === "bigint" || typeof sizeBytes === "string"
      ? sizeBytes
      : undefined,
    createdAt: typeof createdAt === "number" || typeof createdAt === "bigint" || typeof createdAt === "string"
      ? createdAt
      : undefined,
  };
}

export function artifactUriFor(target: SessionArtifactTarget, artifactId: string) {
  return `artifact://${target.ns}/${target.agent}/${target.sessionId}/${artifactId}`;
}

function epochMilliseconds(value: SessionArtifact["createdAt"]): number {
  if (typeof value === "bigint") {
    if (value > BigInt(Number.MAX_SAFE_INTEGER) || value < BigInt(Number.MIN_SAFE_INTEGER)) return 0;
    const numeric = Number(value);
    return numeric >= 1e15 ? Math.trunc(numeric / 1000) : numeric;
  }
  if (typeof value === "number") return value >= 1e15 ? Math.trunc(value / 1000) : value;
  if (typeof value === "string") {
    const numeric = Number(value);
    if (Number.isFinite(numeric)) return numeric >= 1e15 ? Math.trunc(numeric / 1000) : numeric;
    const parsed = Date.parse(value);
    return Number.isFinite(parsed) ? parsed : 0;
  }
  return 0;
}

/** De-duplicate by ID and present the most recently created artifacts first. */
export function mergeSessionArtifacts(existing: SessionArtifact[], incoming: SessionArtifact[]) {
  const byId = new Map<string, SessionArtifact>();
  for (const artifact of [...existing, ...incoming]) byId.set(artifact.id, artifact);
  return Array.from(byId.values()).sort((left, right) => {
    const timestampDifference = epochMilliseconds(right.createdAt) - epochMilliseconds(left.createdAt);
    return timestampDifference || left.id.localeCompare(right.id);
  });
}

export function formatArtifactBytes(value: SessionArtifact["sizeBytes"]) {
  const bytes = typeof value === "bigint" ? Number(value) : Number(value);
  if (!Number.isFinite(bytes) || bytes < 0) return null;
  if (bytes < 1024) return `${Math.trunc(bytes)} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let amount = bytes / 1024;
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit += 1;
  }
  return `${amount >= 10 ? amount.toFixed(0) : amount.toFixed(1)} ${units[unit]}`;
}

export function formatArtifactCreatedAt(value: SessionArtifact["createdAt"]) {
  const milliseconds = epochMilliseconds(value);
  if (!milliseconds || milliseconds < 1e9) return null;
  return new Date(milliseconds).toLocaleString([], { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}
