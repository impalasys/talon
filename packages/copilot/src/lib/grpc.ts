export function normalizeGatewayUrl(url: string) {
  return url.trim().replace(/\/+$/, "");
}

function hasAuthorizationScheme(value: string) {
  return /^(Basic|Bearer)\s+/i.test(value);
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
  const normalizedToken = authToken.trim();
  if (!normalizedToken) return undefined;
  return {
    Authorization: hasAuthorizationScheme(normalizedToken)
      ? normalizedToken
      : `Basic ${base64Encode(`:${normalizedToken}`)}`,
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
