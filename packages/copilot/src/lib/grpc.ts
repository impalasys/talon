export function normalizeGatewayUrl(url: string) {
  return url.trim().replace(/\/+$/, "");
}

export function buildGatewayHeaders(authToken?: string | null) {
  if (!authToken) return undefined;
  return {
    Authorization: `Basic ${btoa(`:${authToken}`)}`,
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
