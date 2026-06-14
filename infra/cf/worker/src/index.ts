import { Container, ContainerProxy } from "@cloudflare/containers";
import { env as workerEnv } from "cloudflare:workers";
import {
  ScheduleShard,
  dispatchQueueBatch,
  handleAlarms,
  handleD1,
  handleQueues,
  handleR2,
  json,
  type TalonCfBindingsEnv,
} from "@impalasys/talon-cf-bindings";

export { ContainerProxy, ScheduleShard };

type WorkerContainerNamespace = DurableObjectNamespace<Container<Env>>;

type Env = Omit<TalonCfBindingsEnv, "WORKER_CONTAINER"> & {
  GATEWAY_CONTAINER: WorkerContainerNamespace;
  WORKER_CONTAINER: WorkerContainerNamespace;
  ENVOY_CONTAINER: WorkerContainerNamespace;
  TALON_GATEWAY_CONTAINER_COUNT?: string;
  TALON_WORKER_CONTAINER_COUNT?: string;
  TALON_ENVOY_CONTAINER_COUNT?: string;
  TALON_CONFIG_INLINE_YAML?: string;
};

const RESERVED_CONTAINER_ENV = new Set([
  "TALON_CONFIG_INLINE_YAML",
  "TALON_GATEWAY_CONTAINER_COUNT",
  "TALON_WORKER_CONTAINER_COUNT",
  "TALON_ENVOY_CONTAINER_COUNT",
]);

const TALON_CONFIG_INLINE_YAML = (
  workerEnv as unknown as { TALON_CONFIG_INLINE_YAML?: string }
).TALON_CONFIG_INLINE_YAML;
const TALON_SCHEDULER_AUTH_TOKEN = (
  workerEnv as unknown as { TALON_SCHEDULER_AUTH_TOKEN?: string }
).TALON_SCHEDULER_AUTH_TOKEN;

function forwardedWorkerEnv(): Record<string, string> {
  const forwarded: Record<string, string> = {};
  for (const [key, value] of Object.entries(workerEnv as Record<string, unknown>)) {
    if (RESERVED_CONTAINER_ENV.has(key)) continue;
    if (typeof value === "string") forwarded[key] = value;
  }
  return forwarded;
}

const FORWARDED_WORKER_ENV = forwardedWorkerEnv();

function configuredCount(raw: string | undefined): number {
  const parsed = Number.parseInt(raw ?? "", 10);
  return Number.isFinite(parsed) && parsed > 0 ? Math.min(parsed, 128) : 1;
}

function stableHash(value: string): number {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}

function instanceName(prefix: string, count: number, key?: string): string {
  if (count <= 1) return "default";
  const shard = key ? stableHash(key) % count : Math.floor(Math.random() * count);
  return `${prefix}-${shard}`;
}

function containerFor(
  namespace: WorkerContainerNamespace,
  prefix: string,
  count: number,
  key?: string,
): DurableObjectStub<Container<Env>> {
  return namespace.get(namespace.idFromName(instanceName(prefix, count, key)));
}

function gatewayContainer(env: Env, key?: string) {
  return containerFor(env.GATEWAY_CONTAINER, "gateway", configuredCount(env.TALON_GATEWAY_CONTAINER_COUNT), key);
}

function workerContainer(env: Env, key?: string) {
  return containerFor(env.WORKER_CONTAINER, "worker", configuredCount(env.TALON_WORKER_CONTAINER_COUNT), key);
}

function envoyContainer(env: Env, key?: string) {
  return containerFor(env.ENVOY_CONTAINER, "envoy", configuredCount(env.TALON_ENVOY_CONTAINER_COUNT), key);
}

function shouldRouteThroughEnvoy(pathname: string): boolean {
  return pathname.startsWith("/v1/") || pathname.startsWith("/talon.gateway.");
}

const CORS_ALLOW_METHODS = "GET,PUT,DELETE,POST,OPTIONS";
const CORS_ALLOW_HEADERS =
  "keep-alive,user-agent,cache-control,content-type,content-transfer-encoding,x-accept-content-transfer-encoding,x-accept-response-streaming,x-user-agent,x-grpc-web,grpc-timeout,connect-protocol-version,connect-timeout-ms,authorization";
const CORS_EXPOSE_HEADERS = "grpc-status,grpc-message";

function corsHeaders(request: Request): Headers {
  const headers = new Headers();
  headers.set("access-control-allow-origin", request.headers.get("origin") ?? "*");
  headers.set("access-control-allow-methods", CORS_ALLOW_METHODS);
  headers.set("access-control-allow-headers", CORS_ALLOW_HEADERS);
  headers.set("access-control-max-age", "1728000");
  headers.set("access-control-expose-headers", CORS_EXPOSE_HEADERS);
  headers.set("access-control-allow-private-network", "true");
  headers.set("vary", "Origin");
  return headers;
}

function withCors(response: Response, request: Request): Response {
  const headers = new Headers(response.headers);
  for (const [key, value] of corsHeaders(request)) {
    headers.set(key, value);
  }
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}

const outboundByHost = {
  "talon-d1.internal": (request: Request, env: Env) => handleD1(request, env),
  "talon-r2.internal": (request: Request, env: Env) => handleR2(request, env),
  "talon-queues.internal": (request: Request, env: Env) => handleQueues(request, env),
  "talon-alarms.internal": (request: Request, env: Env) => handleAlarms(request, env),
  "mock-llm.internal": (request: Request) => {
    const url = new URL(request.url);
    url.hostname = "mock-llm";
    url.port = "8000";
    return fetch(new Request(url.toString(), request));
  },
  "gateway.internal": async (request: Request, env: Env) => {
    const gateway = gatewayContainer(env, new URL(request.url).pathname);
    return gateway.fetch(request);
  },
};

export class GatewayContainer extends Container<Env> {
  defaultPort = 50052;
  requiredPorts = [50051, 50052];
  enableInternet = true;
  envVars = {
    ...FORWARDED_WORKER_ENV,
    GRPC_ADDR: "0.0.0.0:50051",
    GATEWAY_UI_ADDR: "0.0.0.0:50052",
    ...(TALON_CONFIG_INLINE_YAML ? { TALON_CONFIG_INLINE_YAML } : {}),
    TALON_SCHEDULER_DRIVER: "cf_alarms",
    ...(TALON_SCHEDULER_AUTH_TOKEN ? { TALON_SCHEDULER_AUTH_TOKEN } : {}),
  };
  static outboundByHost = outboundByHost;
}

export class WorkerContainer extends Container<Env> {
  defaultPort = 8081;
  enableInternet = true;
  entrypoint = ["talon-worker"];
  envVars = {
    ...FORWARDED_WORKER_ENV,
    PORT: "8081",
    ...(TALON_CONFIG_INLINE_YAML ? { TALON_CONFIG_INLINE_YAML } : {}),
    TALON_SCHEDULER_DRIVER: "cf_alarms",
    ...(TALON_SCHEDULER_AUTH_TOKEN ? { TALON_SCHEDULER_AUTH_TOKEN } : {}),
  };
  static outboundByHost = outboundByHost;
}

export class EnvoyContainer extends Container<Env> {
  defaultPort = 8081;
  enableInternet = false;
  static outboundByHost = outboundByHost;
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (shouldRouteThroughEnvoy(url.pathname)) {
      const envoy = envoyContainer(env);
      return await envoy.fetch(request);
    }

    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: corsHeaders(request) });
    }

    if (url.pathname === "/healthz") {
      await env.TALON_D1.prepare("SELECT 1 AS ok").first();
      return withCors(
        json({
          ok: true,
          containers: {
            gateway: configuredCount(env.TALON_GATEWAY_CONTAINER_COUNT),
            worker: configuredCount(env.TALON_WORKER_CONTAINER_COUNT),
            envoy: configuredCount(env.TALON_ENVOY_CONTAINER_COUNT),
          },
        }),
        request,
      );
    }

    const gateway = gatewayContainer(env);
    return withCors(await gateway.fetch(request), request);
  },

  async queue(batch: MessageBatch, env: Env): Promise<void> {
    await dispatchQueueBatch(batch, env, (message) => workerContainer(env, message.id));
  },
};
