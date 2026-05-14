import { defineConfig, devices } from '@playwright/test';

const API_PORT = process.env.API_PORT || '18789';
const WEB_PORT = process.env.WEB_PORT || '3000';
const PYTHON_BIN = process.env.PYTHON_BIN || 'python3';
const reuseExistingServer = process.env.REUSE_EXISTING_SERVER === 'true'
  ? true
  : process.env.REUSE_EXISTING_SERVER === 'false'
    ? false
    : !process.env.CI;
const DEFAULT_BACKEND_COMMAND = [
  'cd ..',
  'cargo build --locked --bin talon-server --bin talon-worker',
  `PYTHONPATH=.. PATH="$PWD/target/debug:$PATH" ${PYTHON_BIN} tests/run_e2e_stack.py`,
].join(' && ');

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  timeout: process.env.CI ? 120000 : 120000,
  reporter: [['html', { open: 'never' }], ['list']],
  use: {
    baseURL: `http://localhost:${WEB_PORT}`,
    trace: 'on-first-retry',
    actionTimeout: process.env.CI ? 30000 : 0,
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  webServer: [
    {
      command: process.env.BACKEND_COMMAND || DEFAULT_BACKEND_COMMAND,
      url: `http://127.0.0.1:8090/`,
      reuseExistingServer,
      timeout: 240000,
      stdout: 'pipe',
      stderr: 'pipe',
    },
    {
      command: `NEXT_PUBLIC_GATEWAY_URL=http://127.0.0.1:${API_PORT} pnpm dev -p ${WEB_PORT}`,
      url: `http://127.0.0.1:${WEB_PORT}`,
      reuseExistingServer,
      timeout: 60000,
      stdout: 'pipe',
      stderr: 'pipe',
    },
  ],
});
