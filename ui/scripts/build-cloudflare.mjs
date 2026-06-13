import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdir, rename, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(__dirname, '..');
const apiDir = path.join(projectRoot, 'app', 'api');
const disabledRoot = path.join(projectRoot, '.cloudflare-export-disabled');
const disabledApiDir = path.join(disabledRoot, 'api');

let movedApi = false;
let nextBuild;
let cleanupPromise;

async function cleanup() {
  cleanupPromise ??= (async () => {
    if (movedApi) {
      await rename(disabledApiDir, apiDir);
      movedApi = false;
    }
    await rm(disabledRoot, { recursive: true, force: true });
  })();
  return cleanupPromise;
}

async function handleSignal(signal, exitCode) {
  if (nextBuild && !nextBuild.killed) {
    nextBuild.kill(signal);
  }

  try {
    await cleanup();
  } catch (err) {
    console.error('Failed to restore API directory:', err);
  } finally {
    process.exit(exitCode);
  }
}

process.once('SIGINT', () => {
  void handleSignal('SIGINT', 130);
});

process.once('SIGTERM', () => {
  void handleSignal('SIGTERM', 143);
});

try {
  if (existsSync(apiDir)) {
    if (existsSync(disabledApiDir)) {
      throw new Error(`${disabledApiDir} already exists; remove it before building`);
    }
    await mkdir(disabledRoot, { recursive: true });
    await rename(apiDir, disabledApiDir);
    movedApi = true;
  }

  nextBuild = spawn('next', ['build'], {
    cwd: projectRoot,
    env: {
      ...process.env,
      NEXT_OUTPUT: 'export',
      NEXT_PUBLIC_TALON_STATIC_EXPORT: '1',
    },
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });

  const status = await new Promise((resolve, reject) => {
    nextBuild.once('close', resolve);
    nextBuild.once('error', reject);
  });

  if (status !== 0) {
    process.exitCode = status ?? 1;
  }
} finally {
  await cleanup();
}
