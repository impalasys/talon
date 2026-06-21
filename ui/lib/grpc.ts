import { createClient, type Interceptor } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import {
  AuthService,
  ChannelService,
  KnowledgeService,
  NamespaceService,
  ResourceService,
  SessionService,
  WorkflowService,
} from "../proto/proto/talon/v1/api_pb";

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

const createClientset = (url: string) => {
  const transport = createTransport(url);
  return {
    namespaces: createClient(NamespaceService, transport),
    resources: createClient(ResourceService, transport),
    sessions: createClient(SessionService, transport),
    channels: createClient(ChannelService, transport),
    workflows: createClient(WorkflowService, transport),
    knowledge: createClient(KnowledgeService, transport),
    auth: createClient(AuthService, transport),
  };
};

let currentClient = createClientset(process.env.NEXT_PUBLIC_GATEWAY_URL || "http://localhost:50051");

export const getTalonClient = () => currentClient;
export const getGatewayClient = () => currentClient;

export const updateGatewayClient = (url: string) => {
  currentClient = createClientset(url);
};
