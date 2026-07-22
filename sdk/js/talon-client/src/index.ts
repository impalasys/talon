import {
  createPromiseClient,
  type Interceptor,
  type Transport,
} from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import * as config from "./gen/proto/config_pb.js";
import * as data from "./gen/proto/data/data_pb.js";
import * as dataSearch from "./gen/proto/data/search_pb.js";
import * as events from "./gen/proto/events_pb.js";
import * as agents from "./gen/proto/resources/agents_pb.js";
import * as channels from "./gen/proto/resources/channels_pb.js";
import * as common from "./gen/proto/resources/common_pb.js";
import * as deployments from "./gen/proto/resources/deployments_pb.js";
import * as files from "./gen/proto/resources/files_pb.js";
import * as knowledge from "./gen/proto/resources/knowledge_pb.js";
import * as mcp from "./gen/proto/resources/mcp_pb.js";
import * as namespaces from "./gen/proto/resources/namespaces_pb.js";
import * as resources from "./gen/proto/resources/resource_pb.js";
import * as sandboxes from "./gen/proto/resources/sandboxes_pb.js";
import * as schedules from "./gen/proto/resources/schedules_pb.js";
import * as secrets from "./gen/proto/resources/secrets_pb.js";
import * as sessions from "./gen/proto/resources/sessions_pb.js";
import * as usage from "./gen/proto/resources/usage_pb.js";
import * as tasks from "./gen/proto/resources/tasks_pb.js";
import * as workers from "./gen/proto/resources/workers_pb.js";
import * as workflows from "./gen/proto/resources/workflows_pb.js";
import * as v1Auth from "./gen/proto/talon/v1/auth_pb.js";
import * as v1AuthConnect from "./gen/proto/talon/v1/auth_connect.js";
import * as v1Cas from "./gen/proto/talon/v1/cas_pb.js";
import * as v1CasConnect from "./gen/proto/talon/v1/cas_connect.js";
import * as v1Channels from "./gen/proto/talon/v1/channels_pb.js";
import * as v1ChannelsConnect from "./gen/proto/talon/v1/channels_connect.js";
import * as v1Knowledge from "./gen/proto/talon/v1/knowledge_pb.js";
import * as v1KnowledgeConnect from "./gen/proto/talon/v1/knowledge_connect.js";
import * as v1Namespaces from "./gen/proto/talon/v1/namespaces_pb.js";
import * as v1NamespacesConnect from "./gen/proto/talon/v1/namespaces_connect.js";
import * as v1Resources from "./gen/proto/talon/v1/resources_pb.js";
import * as v1ResourcesConnect from "./gen/proto/talon/v1/resources_connect.js";
import * as v1Search from "./gen/proto/talon/v1/search_pb.js";
import * as v1SearchConnect from "./gen/proto/talon/v1/search_connect.js";
import * as v1Sessions from "./gen/proto/talon/v1/sessions_pb.js";
import * as v1SessionsConnect from "./gen/proto/talon/v1/sessions_connect.js";
import * as v1Workflows from "./gen/proto/talon/v1/workflows_pb.js";
import * as v1WorkflowsConnect from "./gen/proto/talon/v1/workflows_connect.js";
import { createTalonClientset, type TalonClient } from "./clientset.js";

export {
  config,
  data,
  dataSearch,
  events,
  agents,
  channels,
  common,
  deployments,
  files,
  knowledge,
  mcp,
  namespaces,
  resources,
  sandboxes,
  schedules,
  secrets,
  sessions,
  tasks,
  usage,
  workers,
  workflows,
  v1Auth,
  v1AuthConnect,
  v1Cas,
  v1CasConnect,
  v1Channels,
  v1ChannelsConnect,
  v1Knowledge,
  v1KnowledgeConnect,
  v1Namespaces,
  v1NamespacesConnect,
  v1Resources,
  v1ResourcesConnect,
  v1Search,
  v1SearchConnect,
  v1Sessions,
  v1SessionsConnect,
  v1Workflows,
  v1WorkflowsConnect,
};
export type { Interceptor, Transport } from "@connectrpc/connect";
export type { TalonClient } from "./clientset.js";

export type TalonClientOptions = {
  baseUrl: string;
  authToken?: string | null;
  /**
   * Server-side only. Long-lived API keys must not be embedded in browser apps.
   */
  apiKey?: string | null;
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
  const baseUrl = options.baseUrl.trim().replace(/\/+$/, "");
  let cachedApiKeyToken: { token: string; expiresAt: number } | undefined;
  let refreshPromise: Promise<string | undefined> | undefined;
  let authClient: TalonClient["auth"] | undefined;

  async function apiKeyAuthorization() {
    const apiKey = options.apiKey?.trim();
    if (!apiKey) return undefined;
    const now = Math.floor(Date.now() / 1000);
    if (cachedApiKeyToken && cachedApiKeyToken.expiresAt > now + 60) {
      return `Bearer ${cachedApiKeyToken.token}`;
    }
    refreshPromise ??= (async () => {
      if (!authClient) {
        const bareTransport = createGrpcWebTransport({
          baseUrl,
          fetch: options.fetch,
          useBinaryFormat: options.useBinaryFormat,
        });
        authClient = createPromiseClient(v1AuthConnect.AuthService, bareTransport);
      }
      const exchanged = await authClient.exchangeApiKey(new v1Auth.ExchangeApiKeyRequest({
        apiKey,
      }));
      cachedApiKeyToken = {
        token: exchanged.accessToken,
        expiresAt: Number(exchanged.expiresAt),
      };
      return `Bearer ${exchanged.accessToken}`;
    })().finally(() => {
      refreshPromise = undefined;
    });
    return await refreshPromise;
  }

  const authInterceptor: Interceptor = (next) => async (req) => {
    const authorization = buildAuthorizationHeader(options.authToken) ?? await apiKeyAuthorization();
    if (authorization) {
      req.header.set("authorization", authorization);
    }
    return await next(req);
  };

  return createGrpcWebTransport({
    baseUrl,
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
