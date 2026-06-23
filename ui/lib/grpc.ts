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

const authInterceptor: Interceptor = (next) => async (req) => {
  if (typeof window !== 'undefined') {
    applyGatewayAuthorizationHeader(req.header, localStorage.getItem('talon_auth_token'));
  }
  return await next(req);
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
