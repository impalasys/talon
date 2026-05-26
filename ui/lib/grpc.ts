import { createClient, type Interceptor } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { GatewayService } from "../proto/proto/gateway_pb";

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

const authInterceptor: Interceptor = (next) => async (req) => {
  if (typeof window !== 'undefined') {
    applyGatewayAuthorizationHeader(req.header, localStorage.getItem('talon_auth_token'));
  }
  return await next(req);
};

const createTransport = (url: string) => createGrpcWebTransport({ 
  baseUrl: normalizeGatewayUrl(url),
  interceptors: [authInterceptor]
});

let currentClient = createClient(
  GatewayService, 
  createTransport(process.env.NEXT_PUBLIC_GATEWAY_URL || "https://envoy.talon.orb.local")
);

export const getGatewayClient = () => currentClient;

export const updateGatewayClient = (url: string) => {
  currentClient = createClient(GatewayService, createTransport(url));
};
