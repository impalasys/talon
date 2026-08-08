function epochMilliseconds(value: unknown) {
  let result: number | null = null;
  if (typeof value === "bigint") {
    if ((value < 0n ? -value : value) > BigInt(Number.MAX_SAFE_INTEGER)) return null;
    result = Number(value);
  } else if (typeof value === "string") {
    const numeric = Number(value);
    result = Number.isFinite(numeric) ? numeric : Date.parse(value);
  } else if (typeof value === "number") result = value;
  if (!Number.isFinite(result) || !result || result < 0) return null;
  if (result >= 1e15) return Math.trunc(result / 1000);
  if (result >= 1e12) return Math.trunc(result);
  return result >= 1e9 ? Math.trunc(result * 1000) : null;
}

export function formatWorkDuration(start: unknown, end: unknown) {
  const startMs = epochMilliseconds(start);
  const endMs = epochMilliseconds(end);
  if (startMs === null || endMs === null || endMs <= startMs) return "Worked";
  const seconds = Math.max(1, Math.round((endMs - startMs) / 1000));
  if (seconds < 60) return `Worked for ${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return seconds % 60 ? `Worked for ${minutes}m ${seconds % 60}s` : `Worked for ${minutes}m`;
}

export function formatWorkingDuration(start: unknown, now = Date.now()) {
  const startMs = epochMilliseconds(start);
  if (startMs === null || now < startMs) return "Working";
  const seconds = Math.max(1, Math.floor((now - startMs) / 1000));
  if (seconds < 60) return `Working for ${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return seconds % 60 ? `Working for ${minutes}m ${seconds % 60}s` : `Working for ${minutes}m`;
}
