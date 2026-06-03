import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import net from "node:net";

export type Provider = {
  name?: string;
  baseUrl: string;
  model: string;
  apiKey: string;
};

export type StartOptions = {
  talonNodePath?: string;
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
    const nodePath = await resolveTalonNode(options.talonNodePath);
    const grpcPort = options.grpcPort ?? await freePort();
    const uiPort = options.uiPort ?? await freePort();
    const tempDir = await mkdtemp(join(tmpdir(), "talon-server-"));
    await mkdir(join(tempDir, "data"), { recursive: true });
    const configPath = join(tempDir, "talon.yaml");
    await writeFile(configPath, configYaml(options.provider), "utf8");
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
    if (!this.child.killed) {
      this.child.kill("SIGINT");
      await Promise.race([
        onceExit(this.child),
        new Promise<void>((resolve) => setTimeout(resolve, 2000)).then(() => {
          if (!this.child.killed) this.child.kill("SIGKILL");
        }),
      ]);
    }
    if (!this.keepTempDir) {
      await rm(this.tempDir, { recursive: true, force: true });
    }
  }
}

export async function start(options: StartOptions = {}): Promise<TalonServer> {
  return TalonServer.start(options);
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

function onceExit(child: ChildProcessWithoutNullStreams): Promise<void> {
  return new Promise((resolve) => child.once("exit", () => resolve()));
}

function configYaml(provider?: Provider): string {
  let yaml = "";
  if (provider) {
    const name = provider.name || "mock";
    yaml += `providers:\n  ${name}:\n    type: openai_compatible\n    base_url: ${JSON.stringify(provider.baseUrl)}\n    model: ${JSON.stringify(provider.model)}\n    api_key: ${JSON.stringify(provider.apiKey)}\ndefault_provider: ${JSON.stringify(name)}\n`;
  }
  yaml += "control_plane:\n  database:\n    driver: sqlite\n    data_dir: ./data\n  message_broker:\n    driver: local_socket\n";
  return yaml;
}

