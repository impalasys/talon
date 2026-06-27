import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";
import net from "node:net";

export type Provider = {
  name?: string;
  baseUrl: string;
  model: string;
  apiKey: string;
};

export type TalonConfig = Record<string, unknown>;

export type StartOptions = {
  talonNodePath?: string;
  configPath?: string;
  config?: TalonConfig;
  dataDir?: string;
  grpcPort?: number;
  uiPort?: number;
  keepTempDir?: boolean;
  env?: Record<string, string>;
  startupTimeoutMs?: number;
  provider?: Provider;
};

export class TalonServer {
  private constructor(
    private readonly child: ChildProcessWithoutNullStreams,
    public readonly tempDir: string,
    public readonly configPath: string,
    private readonly grpcPort: number,
    private readonly uiPort: number,
    private readonly keepTempDir: boolean,
    private readonly logChunks: Buffer[],
  ) {}

  static async start(options: StartOptions = {}): Promise<TalonServer> {
    if (options.configPath && (options.config || options.dataDir || options.provider)) {
      throw new Error("configPath cannot be combined with config, dataDir, or provider; put those settings in the config file");
    }
    if (options.config && options.provider) {
      throw new Error("config cannot be combined with provider; put providers in the config object");
    }
    const nodePath = await resolveTalonNode(options.talonNodePath);
    const grpcPort = options.grpcPort ?? await freePort();
    const uiPort = options.uiPort ?? await freePort();
    const tempDir = await mkdtemp(join(tmpdir(), "talon-server-"));
    let configPath = options.configPath ? resolve(options.configPath) : "";
    if (!configPath) {
      const dataDir = options.dataDir ? resolve(options.dataDir) : undefined;
      const config = options.config
        ? configWithDataDir(options.config, dataDir)
        : defaultConfig(options.provider, dataDir ?? join(tempDir, "data"));
      const configDataDir = controlPlaneDataDir(config);
      if (configDataDir) await mkdir(resolveConfigRelativePath(tempDir, configDataDir), { recursive: true });
      configPath = join(tempDir, "talon.json");
      await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`, "utf8");
    }
    const child = spawn(nodePath, [], {
      env: {
        ...process.env,
        GRPC_ADDR: `127.0.0.1:${grpcPort}`,
        GATEWAY_UI_ADDR: `127.0.0.1:${uiPort}`,
        TALON_CONFIG_PATH: configPath,
        RUST_LOG: "info",
        ...options.env,
      },
    });
    const logs: Buffer[] = [];
    child.stdout.on("data", (chunk: Buffer) => logs.push(chunk));
    child.stderr.on("data", (chunk: Buffer) => logs.push(chunk));
    const server = new TalonServer(child, tempDir, configPath, grpcPort, uiPort, options.keepTempDir ?? false, logs);
    try {
      await waitForPort(grpcPort, options.startupTimeoutMs ?? 30_000);
      return server;
    } catch (error) {
      await server.stop();
      throw new Error(`talon-node did not become ready: ${String(error)}\n${server.logs}`);
    }
  }

  get grpcEndpoint(): string {
    return `127.0.0.1:${this.grpcPort}`;
  }

  get uiEndpoint(): string {
    return `http://127.0.0.1:${this.uiPort}`;
  }

  get logs(): string {
    return Buffer.concat(this.logChunks).toString("utf8");
  }

  async stop(): Promise<void> {
    if (isRunning(this.child)) {
      this.child.kill("SIGINT");
      await Promise.race([
        onceExit(this.child),
        delay(2000),
      ]);
      if (isRunning(this.child)) {
        this.child.kill("SIGKILL");
        await onceExit(this.child);
      }
    }
    if (!this.keepTempDir) {
      await rm(this.tempDir, { recursive: true, force: true });
    }
  }
}

export async function start(options: StartOptions = {}): Promise<TalonServer> {
  return TalonServer.start(options);
}

export function authorizationHeader(token: string): string {
  if (!token.trim()) throw new Error("token is required");
  return `Bearer ${token}`;
}

async function resolveTalonNode(explicit?: string): Promise<string> {
  if (explicit) return explicit;
  if (process.env.TALON_NODE_PATH) return process.env.TALON_NODE_PATH;
  const pkg = platformPackage();
  try {
    const mod = await import(pkg);
    if (typeof mod.talonNodePath === "string") return mod.talonNodePath;
  } catch {
  }
  throw new Error(`talon-node binary not found; install ${pkg} or set TALON_NODE_PATH`);
}

function platformPackage(): string {
  if (process.platform === "linux" && process.arch === "x64") return "@impalasys/talon-node-linux-x64";
  if (process.platform === "darwin" && process.arch === "arm64") return "@impalasys/talon-node-darwin-arm64";
  throw new Error(`unsupported talon-node platform: ${process.platform}-${process.arch}`);
}

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close(() => {
        if (address && typeof address === "object") resolve(address.port);
        else reject(new Error("could not allocate a local port"));
      });
    });
    server.on("error", reject);
  });
}

function waitForPort(port: number, timeoutMs: number): Promise<void> {
  const started = Date.now();
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const socket = net.createConnection({ host: "127.0.0.1", port });
      socket.once("connect", () => {
        socket.destroy();
        resolve();
      });
      socket.once("error", () => {
        socket.destroy();
        if (Date.now() - started > timeoutMs) reject(new Error(`timeout waiting for 127.0.0.1:${port}`));
        else setTimeout(attempt, 100);
      });
    };
    attempt();
  });
}

function isRunning(child: ChildProcessWithoutNullStreams): boolean {
  return child.exitCode === null && child.signalCode === null;
}

function onceExit(child: ChildProcessWithoutNullStreams): Promise<void> {
  if (!isRunning(child)) return Promise.resolve();
  return new Promise((resolve) => child.once("exit", () => resolve()));
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function defaultConfig(provider: Provider | undefined, dataDir: string): TalonConfig {
  const config: TalonConfig = {
    control_plane: {
      database: {
        driver: "sqlite",
        data_dir: dataDir,
      },
      message_broker: {
        driver: "local_socket",
      },
    },
  };
  if (provider) {
    const name = provider.name || "mock";
    config.providers = {
      [name]: {
        type: "openai_compatible",
        base_url: provider.baseUrl,
        model: provider.model,
        api_key: provider.apiKey,
      },
    };
    config.default_provider = name;
  }
  return config;
}

function configWithDataDir(config: TalonConfig, dataDir: string | undefined): TalonConfig {
  const copy = JSON.parse(JSON.stringify(config)) as TalonConfig;
  if (!dataDir) return copy;
  const controlPlane = ensureRecord(copy, "control_plane");
  const database = ensureRecord(controlPlane, "database");
  database.data_dir = dataDir;
  return copy;
}

function ensureRecord(target: Record<string, unknown>, key: string): Record<string, unknown> {
  const current = target[key];
  if (current && typeof current === "object" && !Array.isArray(current)) {
    return current as Record<string, unknown>;
  }
  const value: Record<string, unknown> = {};
  target[key] = value;
  return value;
}

function controlPlaneDataDir(config: TalonConfig): string | undefined {
  const controlPlane = config.control_plane;
  if (!controlPlane || typeof controlPlane !== "object" || Array.isArray(controlPlane)) return undefined;
  const database = (controlPlane as Record<string, unknown>).database;
  if (!database || typeof database !== "object" || Array.isArray(database)) return undefined;
  const dataDir = (database as Record<string, unknown>).data_dir;
  return typeof dataDir === "string" && dataDir.trim() ? dataDir : undefined;
}

function resolveConfigRelativePath(configDir: string, path: string): string {
  return isAbsolute(path) ? path : join(configDir, path);
}
