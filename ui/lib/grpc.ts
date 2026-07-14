import {
  buildAuthorizationHeader,
  createTalonClient,
  type Interceptor,
  type TalonClient,
} from "@impalasys/talon-client";

export function normalizeGatewayUrl(url: string) {
  return url.trim().replace(/\/+$/, "");
}

export function getDefaultGatewayUrl() {
  const configuredUrl = process.env.NEXT_PUBLIC_GATEWAY_URL?.trim();
  if (configuredUrl) {
    return normalizeGatewayUrl(configuredUrl);
  }

  if (typeof window !== "undefined" && window.location.protocol === "https:") {
    if (window.location.hostname.startsWith("ui.")) {
      const gatewayUrl = new URL(window.location.href);
      gatewayUrl.hostname = gatewayUrl.hostname.replace(/^ui\./, "gateway.");
      gatewayUrl.port = "";
      gatewayUrl.pathname = "";
      gatewayUrl.search = "";
      gatewayUrl.hash = "";
      return gatewayUrl.origin;
    }
    return window.location.origin;
  }

  return "http://localhost:50051";
}

export function isBlockedMixedContentGatewayUrl(url: string) {
  if (typeof window === "undefined" || window.location.protocol !== "https:") {
    return false;
  }

  try {
    return new URL(normalizeGatewayUrl(url), window.location.href).protocol === "http:";
  } catch {
    return false;
  }
}

export function buildGatewayHeaders(authToken?: string | null) {
  const authorization = buildAuthorizationHeader(authToken);
  if (!authorization) return undefined;
  return {
    Authorization: authorization,
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

export const TALON_AUTH_EXPIRED_EVENT = "talon-auth-expired";
const RUNTIME_AUTH_TOKEN_STORAGE_KEY = "talon_auth_token";
const SIGHTLINE_REFRESH_URL_COOKIE_NAME = "sightline_refresh_url";

type SightlineRefreshResponse = {
  accessToken?: string;
  tokenType?: string;
  expiresIn?: number;
  expiresAt?: number;
  gatewayUrl?: string;
  namespace?: string;
};

let sightlineRefreshPromise: Promise<string | null> | null = null;

function delay(ms: number) {
  return new Promise<void>((resolve) => window.setTimeout(resolve, ms));
}

export function isExpiredSignatureAuthError(error: unknown) {
  const candidate = error as {
    message?: string;
    rawMessage?: string;
    code?: string | number;
    codeName?: string;
    cause?: { message?: string };
  };
  const message = `${candidate?.rawMessage || candidate?.message || candidate?.cause?.message || ""}`.toLowerCase();
  const code = `${candidate?.codeName || candidate?.code || ""}`.toLowerCase();
  return (
    message.includes("expired signature") ||
    message.includes("signature has expired") ||
    (code.includes("unauthenticated") && message.includes("expired"))
  );
}

export function getSightlineRefreshUrl() {
  if (typeof document === "undefined") return null;
  for (const cookie of document.cookie.split(";")) {
    const [rawName, ...rawValueParts] = cookie.split("=");
    if (rawName?.trim() !== SIGHTLINE_REFRESH_URL_COOKIE_NAME) continue;
    const rawValue = rawValueParts.join("=").trim();
    if (!rawValue) return null;
    try {
      return decodeURIComponent(rawValue);
    } catch {
      return rawValue;
    }
  }
  return null;
}

async function refreshSightlineAuthToken() {
  if (typeof window === "undefined") return null;
  const refreshUrl = getSightlineRefreshUrl();
  if (!refreshUrl) return null;
  sightlineRefreshPromise ??= (async () => {
    try {
      const response = await fetch(refreshUrl, {
        method: "POST",
        credentials: "include",
        headers: {
          "content-type": "application/json",
        },
      });
      if (!response.ok) {
        return null;
      }
      const payload = await response.json().catch(() => ({})) as SightlineRefreshResponse;
      const nextToken = payload.accessToken?.trim();
      if (!nextToken) return null;
      try {
        localStorage.setItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY, nextToken);
      } catch {
        // Keep the fresh token usable for the retry even when storage is unavailable.
      }
      return nextToken;
    } catch {
      return null;
    }
  })().finally(() => {
    sightlineRefreshPromise = null;
  });
  return sightlineRefreshPromise;
}

const authInterceptor: Interceptor = (next) => async (req) => {
  let attemptedToken: string | null = null;
  if (typeof window !== 'undefined') {
    attemptedToken = localStorage.getItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY);
    applyGatewayAuthorizationHeader(req.header, attemptedToken);
  }
  try {
    return await next(req);
  } catch (error) {
    if (typeof window !== 'undefined' && isExpiredSignatureAuthError(error)) {
      const refreshedToken = await refreshSightlineAuthToken();
      if (refreshedToken) {
        applyGatewayAuthorizationHeader(req.header, refreshedToken);
        return await next(req);
      }
      await delay(500);
      const currentToken = localStorage.getItem(RUNTIME_AUTH_TOKEN_STORAGE_KEY);
      if (currentToken && currentToken !== attemptedToken) {
        applyGatewayAuthorizationHeader(req.header, currentToken);
        return await next(req);
      }
      window.dispatchEvent(new CustomEvent(TALON_AUTH_EXPIRED_EVENT));
    }
    throw error;
  }
};

const createClientset = (url: string): TalonClient => createTalonClient({
  baseUrl: normalizeGatewayUrl(url),
  interceptors: [authInterceptor],
});

let currentClient = createClientset(getDefaultGatewayUrl());

export const getTalonClient = () => currentClient;
export const getGatewayClient = () => currentClient;

export const updateGatewayClient = (url: string) => {
  currentClient = createClientset(url);
};
