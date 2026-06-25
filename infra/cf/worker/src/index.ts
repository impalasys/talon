import {
  Container,
  ContainerProxy,
  switchPort,
  type ContainerStartConfigOptions,
} from "@cloudflare/containers";
import {
  ScheduleShard,
  SessionStreamShard,
  dispatchQueueBatch,
  handleAlarms,
  handleD1,
  handleQueues,
  handleR2,
  json,
  TEXT_JSON,
  type TalonCfBindingsEnv,
} from "@impalasys/talon-cf-bindings";

export { ContainerProxy, ScheduleShard, SessionStreamShard };

type WorkerContainerNamespace = DurableObjectNamespace<Container<Env>>;

type Env = Omit<TalonCfBindingsEnv, "WORKER_CONTAINER"> & {
  GATEWAY_CONTAINER: WorkerContainerNamespace;
  WORKER_CONTAINER: WorkerContainerNamespace;
  TALON_GATEWAY_CONTAINER_COUNT?: string;
  TALON_WORKER_CONTAINER_COUNT?: string;
  TALON_CONFIG_INLINE_YAML?: string;
  TALON_SCHEDULER_AUTH_TOKEN?: string;
  TALON_CF_DEV_EXTERNAL_CONTAINERS?: string;
  TALON_CF_DEV_GATEWAY_URL?: string;
  TALON_CF_DEV_GATEWAY_GRPC_URL?: string;
  TALON_CF_DEV_WORKER_URL?: string;
};

const RESERVED_CONTAINER_ENV = new Set([
  "TALON_CONFIG_INLINE_YAML",
  "TALON_GATEWAY_CONTAINER_COUNT",
  "TALON_WORKER_CONTAINER_COUNT",
]);

function forwardedWorkerEnv(env: Env): Record<string, string> {
  const forwarded: Record<string, string> = {};
  for (const [key, value] of Object.entries(env as Record<string, unknown>)) {
    if (RESERVED_CONTAINER_ENV.has(key)) continue;
    if (typeof value === "string") forwarded[key] = value;
  }
  return forwarded;
}

const CONTAINER_START_OPTIONS = {
  instanceGetTimeoutMS: 10_000,
  portReadyTimeoutMS: 15_000,
  waitInterval: 500,
};
type ContainerStartProfile = {
  ports: number[];
  startOptions: ContainerStartConfigOptions;
};
type ContainerReadiness = {
  ready: boolean;
  status: number;
  error?: string;
};
type ContainerStartAndWaitOptions = {
  startOptions: ContainerStartConfigOptions;
  ports: number[];
  cancellationOptions: typeof CONTAINER_START_OPTIONS;
};

const GATEWAY_CONTAINER_PORTS = [50051];
const WORKER_CONTAINER_PORTS = [8081];

const GATEWAY_CONTAINER_ENTRYPOINT = ["/usr/local/bin/talon-server"];
const WORKER_CONTAINER_ENTRYPOINT = ["/usr/local/bin/talon-worker"];

function gatewayContainerStartProfile(env: Env): ContainerStartProfile {
  return {
    ports: GATEWAY_CONTAINER_PORTS,
    startOptions: {
      entrypoint: GATEWAY_CONTAINER_ENTRYPOINT,
      enableInternet: true,
      envVars: {
        ...forwardedWorkerEnv(env),
        GRPC_ADDR: "0.0.0.0:50051",
        ...(env.TALON_CONFIG_INLINE_YAML ? { TALON_CONFIG_INLINE_YAML: env.TALON_CONFIG_INLINE_YAML } : {}),
        TALON_SCHEDULER_DRIVER: "cf_alarms",
        ...(env.TALON_SCHEDULER_AUTH_TOKEN ? { TALON_SCHEDULER_AUTH_TOKEN: env.TALON_SCHEDULER_AUTH_TOKEN } : {}),
      },
    },
  };
}

function workerContainerStartProfile(env: Env, instance: string): ContainerStartProfile {
  return {
    ports: WORKER_CONTAINER_PORTS,
    startOptions: {
      entrypoint: WORKER_CONTAINER_ENTRYPOINT,
      enableInternet: true,
      envVars: {
        ...forwardedWorkerEnv(env),
        PORT: "8081",
        TALON_CLOUDFLARE_CONTAINER_NAME: instance,
        TALON_WORKER_ENDPOINT_URL: workerContainerInternalUrl(instance),
        ...(env.TALON_CONFIG_INLINE_YAML ? { TALON_CONFIG_INLINE_YAML: env.TALON_CONFIG_INLINE_YAML } : {}),
        TALON_SCHEDULER_DRIVER: "cf_alarms",
        ...(env.TALON_SCHEDULER_AUTH_TOKEN ? { TALON_SCHEDULER_AUTH_TOKEN: env.TALON_SCHEDULER_AUTH_TOKEN } : {}),
      },
    },
  };
}

const GATEWAY_CONTAINER_DEFAULT_START_OPTIONS = {
  entrypoint: GATEWAY_CONTAINER_ENTRYPOINT,
  enableInternet: true,
} satisfies ContainerStartConfigOptions;
const WORKER_CONTAINER_DEFAULT_START_OPTIONS = {
  entrypoint: WORKER_CONTAINER_ENTRYPOINT,
  enableInternet: true,
} satisfies ContainerStartConfigOptions;

function startAndWaitOptions(profile: ContainerStartProfile): ContainerStartAndWaitOptions {
  return {
    ...profile,
    cancellationOptions: CONTAINER_START_OPTIONS,
  };
}

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
  name: string,
): DurableObjectStub<Container<Env>> {
  return namespace.get(namespace.idFromName(name));
}

function gatewayContainer(env: Env, key?: string) {
  return containerFor(env.GATEWAY_CONTAINER, instanceName("gateway", configuredCount(env.TALON_GATEWAY_CONTAINER_COUNT), key));
}

function workerContainerName(env: Env, key?: string): string {
  return instanceName("worker", configuredCount(env.TALON_WORKER_CONTAINER_COUNT), key);
}

function workerContainerByName(env: Env, name: string) {
  return containerFor(env.WORKER_CONTAINER, name);
}

function workerContainerRef(env: Env, key?: string): { name: string; container: DurableObjectStub<Container<Env>> } {
  const name = workerContainerName(env, key);
  return { name, container: workerContainerByName(env, name) };
}

function workerContainer(env: Env, key?: string) {
  return workerContainerRef(env, key).container;
}

function externalContainersEnabled(env: Env): boolean {
  return env.TALON_CF_DEV_EXTERNAL_CONTAINERS === "true";
}

function workerContainerInternalUrl(instance: string): string {
  return `http://${instance}.worker.internal`;
}

function workerInstanceFromHost(hostname: string): string | undefined {
  const suffix = ".worker.internal";
  if (!hostname.endsWith(suffix)) return undefined;
  const instance = hostname.slice(0, -suffix.length);
  if (instance === "default" || /^worker-\d+$/.test(instance)) return instance;
  return undefined;
}

function requestToOrigin(request: Request, origin: string): Request {
  const target = new URL(request.url);
  const originUrl = new URL(origin);
  target.protocol = originUrl.protocol;
  target.host = originUrl.host;
  return new Request(target.toString(), request);
}

function fetcherForOrigin(origin: string): { fetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> } {
  return {
    fetch(input, init) {
      const request = new Request(input, init);
      return fetch(requestToOrigin(request, origin));
    },
  };
}

async function containerReady(
  name: string,
  container: DurableObjectStub<Container<Env>>,
  request: Request,
  profile: ContainerStartProfile,
): Promise<ContainerReadiness> {
  try {
    await container.startAndWaitForPorts(startAndWaitOptions(profile));
    const response = await container.fetch(request);
    const ready = response.status < 500;
    if (!ready) {
      const text = await response.clone().text().catch(() => "");
      console.error(`${name} container readiness probe failed`, {
        status: response.status,
        body: text.slice(0, 500),
      });
    }
    return { ready, status: response.status };
  } catch (error) {
    console.error(`${name} container readiness probe threw`, error);
    return { ready: false, status: 0, error: error instanceof Error ? error.message : String(error) };
  }
}

async function fetchStartedContainer(
  container: DurableObjectStub<Container<Env>>,
  request: Request,
  profile: ContainerStartProfile,
): Promise<Response> {
  await container.startAndWaitForPorts(startAndWaitOptions(profile));
  return container.fetch(request);
}

function requestPort(request: Request): number | undefined {
  const port = Number(new URL(request.url).port);
  return Number.isInteger(port) && port > 0 ? port : undefined;
}

async function serviceReady(origin: string, path: string, init?: RequestInit): Promise<boolean> {
  try {
    const response = await fetch(new URL(path, origin), init);
    return response.status < 500;
  } catch {
    return false;
  }
}

function gatewayGrpcRequest(request: Request): boolean {
  const url = new URL(request.url);
  if (url.pathname.startsWith("/talon.v1.")) return true;
  const contentType = request.headers.get("content-type")?.toLowerCase() ?? "";
  return contentType.startsWith("application/grpc") || contentType.startsWith("application/grpc-web");
}

function gatewayPort(request: Request): number {
  return 50051;
}

async function fetchGateway(env: Env, request: Request): Promise<Response> {
  const gateway = gatewayContainer(env, new URL(request.url).pathname);
  return fetchStartedContainer(
    gateway,
    switchPort(request, gatewayPort(request)),
    gatewayContainerStartProfile(env),
  );
}

async function fetchWorkerInstance(env: Env, instance: string, request: Request): Promise<Response> {
  return fetchStartedContainer(
    workerContainerByName(env, instance),
    switchPort(request, WORKER_CONTAINER_PORTS[0]),
    workerContainerStartProfile(env, instance),
  );
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

const outboundByHost: Record<string, (request: Request, env: Env) => Promise<Response> | Response> = {
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
    const port = requestPort(request);
    return fetchStartedContainer(
      gateway,
      port ? switchPort(request, port) : request,
      gatewayContainerStartProfile(env),
    );
  },
};

for (const instance of ["default", ...Array.from({ length: 128 }, (_, index) => `worker-${index}`)]) {
  outboundByHost[`${instance}.worker.internal`] = (request: Request, env: Env) =>
    fetchWorkerInstance(env, instance, request);
}

export class GatewayContainer extends Container<Env> {
  defaultPort = 50051;
  requiredPorts = GATEWAY_CONTAINER_PORTS;
  enableInternet = GATEWAY_CONTAINER_DEFAULT_START_OPTIONS.enableInternet;
  entrypoint = GATEWAY_CONTAINER_DEFAULT_START_OPTIONS.entrypoint;
  // Rust processes call internal hostnames during bootstrap; install outbound handlers before start().
  usingInterception = true;
}

export class WorkerContainer extends Container<Env> {
  defaultPort = 8081;
  requiredPorts = WORKER_CONTAINER_PORTS;
  enableInternet = WORKER_CONTAINER_DEFAULT_START_OPTIONS.enableInternet;
  entrypoint = WORKER_CONTAINER_DEFAULT_START_OPTIONS.entrypoint;
  usingInterception = true;
}

// Assign after class declarations so @cloudflare/containers' inherited static
// setter registers these handlers for ContainerProxy.
GatewayContainer.outboundByHost = outboundByHost;
WorkerContainer.outboundByHost = outboundByHost;

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: corsHeaders(request) });
    }

    const outboundHandler = outboundByHost[url.hostname as keyof typeof outboundByHost];
    if (outboundHandler) {
      return withCors(await outboundHandler(request, env), request);
    }
    const workerInstance = workerInstanceFromHost(url.hostname);
    if (workerInstance) {
      return withCors(await fetchWorkerInstance(env, workerInstance, request), request);
    }

    if (url.pathname === "/healthz") {
      await env.TALON_D1.prepare("SELECT 1 AS ok").first();
      if (externalContainersEnabled(env)) {
        const gatewayUrl = env.TALON_CF_DEV_GATEWAY_URL ?? "http://gateway:50051";
        const workerUrl = env.TALON_CF_DEV_WORKER_URL ?? "http://worker:8081";
        const [gatewayReady, workerReady] = await Promise.all([
          serviceReady(gatewayUrl, "/"),
          serviceReady(workerUrl, "/cloudflare/queues/dispatch", {
            method: "POST",
            headers: TEXT_JSON,
            body: "{}",
          }),
        ]);
        const ok = gatewayReady && workerReady;
        return withCors(
          json({
            ok,
            mode: "external-compose",
            services: {
              gateway: gatewayReady,
              worker: workerReady,
            },
          }, { status: ok ? 200 : 503 }),
          request,
        );
      }
      const [gateway, worker] = await Promise.all([
        containerReady(
          "gateway",
          gatewayContainer(env),
          new Request("https://talon-health.internal/"),
          gatewayContainerStartProfile(env),
        ),
        (() => {
          const worker = workerContainerRef(env);
          return containerReady(
            "worker",
            worker.container,
            new Request("https://talon-health.internal/cloudflare/queues/dispatch", {
              method: "POST",
              headers: TEXT_JSON,
              body: "{}",
            }),
            workerContainerStartProfile(env, worker.name),
          );
        })(),
      ]);
      const ok = gateway.ready && worker.ready;
      return withCors(
        json({
          ok,
          containers: {
            gateway: {
              count: configuredCount(env.TALON_GATEWAY_CONTAINER_COUNT),
              ready: gateway.ready,
              status: gateway.status,
              error: gateway.error,
            },
            worker: {
              count: configuredCount(env.TALON_WORKER_CONTAINER_COUNT),
              ready: worker.ready,
              status: worker.status,
              error: worker.error,
            },
          },
        }, { status: ok ? 200 : 503 }),
        request,
      );
    }

    if (externalContainersEnabled(env)) {
      const origin = env.TALON_CF_DEV_GATEWAY_GRPC_URL ?? env.TALON_CF_DEV_GATEWAY_URL ?? "http://gateway:50051";
      return withCors(await fetch(requestToOrigin(request, origin)), request);
    }

    return withCors(await fetchGateway(env, request), request);
  },

  async queue(batch: MessageBatch, env: Env): Promise<void> {
    await dispatchQueueBatch(batch, env, (message) => {
      if (externalContainersEnabled(env)) {
        return fetcherForOrigin(env.TALON_CF_DEV_WORKER_URL ?? "http://worker:8081");
      }
      const { name, container } = workerContainerRef(env, message.id);
      return {
        fetch(input: RequestInfo | URL, init?: RequestInit) {
          return fetchStartedContainer(
            container,
            new Request(input, init),
            workerContainerStartProfile(env, name),
          );
        },
      };
    });
  },
};
