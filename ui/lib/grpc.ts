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

const authInterceptor: Interceptor = (next) => async (req) => {
  if (typeof window !== 'undefined') {
    const headers = buildGatewayHeaders(localStorage.getItem('talon_auth_token'));
    if (headers?.Authorization) {
      req.header.set("authorization", headers.Authorization);
    }
  }
  return await next(req);
};

const createTransport = (url: string) => createGrpcWebTransport({ 
  baseUrl: normalizeGatewayUrl(url),
  interceptors: [authInterceptor]
});

let currentClient = createClient(
  GatewayService, 
  createTransport(process.env.NEXT_PUBLIC_GATEWAY_URL || "http://envoy.talon.orb.local")
);

export const getGatewayClient = () => currentClient;

export const updateGatewayClient = (url: string) => {
  currentClient = createClient(GatewayService, createTransport(url));
};
