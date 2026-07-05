import { defineConfig, devices } from '@playwright/test';
import fs from 'fs';

const API_PORT = process.env.API_PORT || '50051';
const WEB_PORT = process.env.WEB_PORT || '3000';
const E2E_READY_PORT = process.env.READY_PORT || process.env.E2E_READY_PORT || '8090';
const SETUP_PYTHON_BIN = process.env.pythonLocation
  ? `${process.env.pythonLocation}/bin/python`
  : null;
const DEFAULT_PYTHON_BIN = process.env.CI
  ? (SETUP_PYTHON_BIN || 'python3')
  : (fs.existsSync('/usr/bin/python3') ? '/usr/bin/python3' : 'python3');
const PYTHON_BIN = process.env.PYTHON_BIN || DEFAULT_PYTHON_BIN;
const reuseExistingServer = process.env.REUSE_EXISTING_SERVER === 'true'
  ? true
  : process.env.REUSE_EXISTING_SERVER === 'false'
    ? false
    : !process.env.CI;
const DEFAULT_BACKEND_COMMAND = [
  'cd ..',
  'if [ ! -x target/debug/talon-server ] || [ ! -x target/debug/talon-worker ] || [ ! -x target/debug/talon-cli ]; then cargo build --locked --bin talon-server --bin talon-worker --bin talon-cli; fi',
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
    video: process.env.CAPTURE_E2E_VIDEO === 'true' ? 'on' : 'off',
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
      url: `http://127.0.0.1:${E2E_READY_PORT}/`,
      reuseExistingServer,
      timeout: 600000,
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
