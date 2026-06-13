import { TEXT_JSON } from "./constants";

export function json(data: unknown, init: ResponseInit = {}) {
  return Response.json(data, { ...init, headers: { ...TEXT_JSON, ...init.headers } });
}

export async function body<T>(request: Request): Promise<T> {
  return (await request.json()) as T;
}

export function decodeBase64(value: string): Uint8Array {
  return Uint8Array.from(atob(value), (char) => char.charCodeAt(0));
}

export function encodeBase64(value: ArrayBuffer | Uint8Array | null): string | null {
  if (value === null) return null;
  const bytes = value instanceof Uint8Array ? value : new Uint8Array(value);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export function nowMicros(): number {
  return Date.now() * 1000;
}
