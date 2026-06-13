import { spawnSync } from 'node:child_process';
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

try {
  if (existsSync(apiDir)) {
    if (existsSync(disabledApiDir)) {
      throw new Error(`${disabledApiDir} already exists; remove it before building`);
    }
    await mkdir(disabledRoot, { recursive: true });
    await rename(apiDir, disabledApiDir);
    movedApi = true;
  }

  const result = spawnSync('next', ['build'], {
    cwd: projectRoot,
    env: {
      ...process.env,
      NEXT_OUTPUT: 'export',
      NEXT_PUBLIC_TALON_STATIC_EXPORT: '1',
    },
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    process.exitCode = result.status ?? 1;
  }
} finally {
  if (movedApi) {
    await rename(disabledApiDir, apiDir);
  }
  await rm(disabledRoot, { recursive: true, force: true });
}
