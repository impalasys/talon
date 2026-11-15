import { defineConfig, devices } from '@playwright/test';

const WEB_PORT = process.env.WEB_PORT || '3000';
const E2E_READY_PORT = process.env.READY_PORT || process.env.E2E_READY_PORT || '8090';
const reuseExistingServer = process.env.REUSE_EXISTING_SERVER === 'true';
const DEFAULT_BACKEND_COMMAND = 'cd .. && bazel run //tests:e2e_stack';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  timeout: process.env.CI ? 120000 : 120000,
  reporter: [['html', { open: 'never', outputFolder: process.env.TEST_UNDECLARED_OUTPUTS_DIR ? `${process.env.TEST_UNDECLARED_OUTPUTS_DIR}/report` : 'playwright-report' }], ['list']],
  outputDir: process.env.TEST_UNDECLARED_OUTPUTS_DIR ? `${process.env.TEST_UNDECLARED_OUTPUTS_DIR}/results` : 'test-results',
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
      command: process.env.FRONTEND_COMMAND || 'cd .. && bazel run //ui:preview',
      url: `http://127.0.0.1:${WEB_PORT}`,
      reuseExistingServer,
      timeout: 60000,
      stdout: 'pipe',
      stderr: 'pipe',
    },
  ],
});
