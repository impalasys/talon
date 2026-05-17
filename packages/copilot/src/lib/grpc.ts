export function normalizeGatewayUrl(url: string) {
  return url.trim().replace(/\/+$/, "");
}

function base64Encode(value: string) {
  if (typeof TextEncoder !== "undefined" && typeof btoa === "function") {
    const bytes = new TextEncoder().encode(value);
    let binary = "";
    for (const byte of bytes) {
      binary += String.fromCharCode(byte);
    }
    return btoa(binary);
  }
  if (typeof btoa === "function") {
    return btoa(value);
  }
  if (typeof Buffer !== "undefined") {
    return Buffer.from(value, "utf-8").toString("base64");
  }
  throw new Error("No base64 encoder available in this environment.");
}

export function buildGatewayHeaders(authToken?: string | null) {
  if (!authToken) return undefined;
  return {
    Authorization: `Basic ${base64Encode(`:${authToken}`)}`,
  };
}

export function applyGatewayAuthorizationHeader(
  headerTarget: { set(name: string, value: string): void },
  authToken?: string | null,
) {
  const headers = buildGatewayHeaders(authToken);
  if (headers?.Authorization) {
    headerTarget.set("authorization", headers.Authorization);
  }
}
