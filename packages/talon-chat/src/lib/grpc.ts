export function normalizeGatewayUrl(url: string) {
  return url.trim().replace(/\/+$/, "");
}

function hasAuthorizationScheme(value: string) {
  return /^(Basic|Bearer)\s+/i.test(value);
}

export function buildGatewayHeaders(authToken?: string | null) {
  if (!authToken) return undefined;
  const normalizedToken = authToken.trim();
  if (!normalizedToken) return undefined;
  return {
    Authorization: hasAuthorizationScheme(normalizedToken)
      ? normalizedToken
      : `Bearer ${normalizedToken}`,
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
