import { TEXT_JSON } from "./constants";

type BufferLike = Uint8Array & {
  toString(encoding: "base64"): string;
};

declare const Buffer: {
  from(value: string, encoding: "base64"): Uint8Array;
  from(value: ArrayBuffer | Uint8Array): BufferLike;
};

export function json(data: unknown, init: ResponseInit = {}) {
  const headers = new Headers(init.headers);
  for (const [key, value] of Object.entries(TEXT_JSON)) {
    if (!headers.has(key)) headers.set(key, value);
  }
  return Response.json(data, { ...init, headers });
}

export async function body<T>(request: Request): Promise<T> {
  return (await request.json()) as T;
}

export function decodeBase64(value: string): Uint8Array {
  return Buffer.from(value, "base64");
}

export function encodeBase64(value: ArrayBuffer | Uint8Array | null): string | null {
  if (value === null) return null;
  return Buffer.from(value).toString("base64");
}

export function nowMicros(): number {
  return Date.now() * 1000;
}
