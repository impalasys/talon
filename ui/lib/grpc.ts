import {
  buildAuthorizationHeader,
  createTalonClient,
  type Interceptor,
  type TalonClient,
} from "@impalasys/talon-client";

export function normalizeGatewayUrl(url: string) {
  return url.trim().replace(/\/+$/, "");
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

let currentClient = createClientset(process.env.NEXT_PUBLIC_GATEWAY_URL || "http://localhost:50051");

export const getTalonClient = () => currentClient;
export const getGatewayClient = () => currentClient;

export const updateGatewayClient = (url: string) => {
  currentClient = createClientset(url);
};
