import { defineConfig, devices } from '@playwright/test';

const API_PORT = process.env.API_PORT || '18789';
const WEB_PORT = process.env.WEB_PORT || '3000';

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
      command: process.env.BACKEND_COMMAND || `bazel build //talon:talon_server //talon:talon_worker && bazel run //talon/tests:run_e2e_stack`,
      url: `http://127.0.0.1:8090/`,
      reuseExistingServer: !process.env.CI || process.env.REUSE_EXISTING_SERVER === 'true',
      timeout: 120000,
      stdout: 'pipe',
      stderr: 'pipe',
    },
    {
      command: `NEXT_PUBLIC_GATEWAY_URL=http://127.0.0.1:${API_PORT} pnpm dev -p ${WEB_PORT}`,
      url: `http://127.0.0.1:${WEB_PORT}`,
      reuseExistingServer: !process.env.CI || process.env.REUSE_EXISTING_SERVER === 'true',
      timeout: 60000,
      stdout: 'pipe',
      stderr: 'pipe',
    },
  ],
});
