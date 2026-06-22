import {
  type Interceptor,
  type Transport,
} from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { createTalonClientset, type TalonClient } from "./clientset.js";

export * as config from "./gen/proto/config_pb.js";
export * as data from "./gen/proto/data/data_pb.js";
export * as events from "./gen/proto/events_pb.js";
export * as agents from "./gen/proto/resources/agents_pb.js";
export * as channels from "./gen/proto/resources/channels_pb.js";
export * as common from "./gen/proto/resources/common_pb.js";
export * as deployments from "./gen/proto/resources/deployments_pb.js";
export * as knowledge from "./gen/proto/resources/knowledge_pb.js";
export * as mcp from "./gen/proto/resources/mcp_pb.js";
export * as namespaces from "./gen/proto/resources/namespaces_pb.js";
export * as resources from "./gen/proto/resources/resource_pb.js";
export * as sandboxes from "./gen/proto/resources/sandboxes_pb.js";
export * as schedules from "./gen/proto/resources/schedules_pb.js";
export * as sessions from "./gen/proto/resources/sessions_pb.js";
export * as usage from "./gen/proto/resources/usage_pb.js";
export * as workers from "./gen/proto/resources/workers_pb.js";
export * as workflows from "./gen/proto/resources/workflows_pb.js";
export * as v1Auth from "./gen/proto/talon/v1/auth_pb.js";
export * as v1AuthConnect from "./gen/proto/talon/v1/auth_connect.js";
export * as v1Channels from "./gen/proto/talon/v1/channels_pb.js";
export * as v1ChannelsConnect from "./gen/proto/talon/v1/channels_connect.js";
export * as v1Knowledge from "./gen/proto/talon/v1/knowledge_pb.js";
export * as v1KnowledgeConnect from "./gen/proto/talon/v1/knowledge_connect.js";
export * as v1Namespaces from "./gen/proto/talon/v1/namespaces_pb.js";
export * as v1NamespacesConnect from "./gen/proto/talon/v1/namespaces_connect.js";
export * as v1Resources from "./gen/proto/talon/v1/resources_pb.js";
export * as v1ResourcesConnect from "./gen/proto/talon/v1/resources_connect.js";
export * as v1Sessions from "./gen/proto/talon/v1/sessions_pb.js";
export * as v1SessionsConnect from "./gen/proto/talon/v1/sessions_connect.js";
export * as v1Workflows from "./gen/proto/talon/v1/workflows_pb.js";
export * as v1WorkflowsConnect from "./gen/proto/talon/v1/workflows_connect.js";
export type { TalonClient } from "./clientset.js";

export type TalonClientOptions = {
  baseUrl: string;
  authToken?: string | null;
  fetch?: typeof globalThis.fetch;
  interceptors?: Interceptor[];
  useBinaryFormat?: boolean;
};

function hasAuthorizationScheme(value: string) {
  return /^(Basic|Bearer)\s+/i.test(value);
}

export function buildAuthorizationHeader(authToken?: string | null) {
  if (!authToken) return undefined;
  const normalizedToken = authToken.trim();
  if (!normalizedToken) return undefined;
  return hasAuthorizationScheme(normalizedToken)
    ? normalizedToken
    : `Bearer ${normalizedToken}`;
}

export function createTalonTransport(options: TalonClientOptions): Transport {
  if (!options || typeof options.baseUrl !== "string" || !options.baseUrl.trim()) {
    throw new Error("TalonClient requires a baseUrl.");
  }

  const authInterceptor: Interceptor = (next) => async (req) => {
    const authorization = buildAuthorizationHeader(options.authToken);
    if (authorization) {
      req.header.set("authorization", authorization);
    }
    return await next(req);
  };

  return createGrpcWebTransport({
    baseUrl: options.baseUrl.trim().replace(/\/+$/, ""),
    fetch: options.fetch,
    interceptors: [authInterceptor, ...(options.interceptors ?? [])],
    useBinaryFormat: options.useBinaryFormat,
  });
}

export function createTalonClient(options: string | TalonClientOptions): TalonClient {
  const resolved = typeof options === "string" ? { baseUrl: options } : options;
  const transport = createTalonTransport(resolved);
  return createTalonClientset(transport);
}
