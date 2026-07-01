import { test, expect } from '@playwright/test';
import { createHmac } from 'node:crypto';

function base64Url(value: object) {
  return Buffer.from(JSON.stringify(value)).toString('base64url');
}

function createLocalRootToken() {
  const header = base64Url({ typ: 'JWT', alg: 'HS256' });
  const payload = base64Url({
    sub: 'talon-root-client',
    aud: 'talon',
    exp: Math.floor(Date.now() / 1000) + 3600,
  });
  const body = `${header}.${payload}`;
  const signature = createHmac('sha256', 'local-dev-talon-jwt').update(body).digest('base64url');
  return `${body}.${signature}`;
}

test.describe('Talon UI', () => {
  test('shows a connection error when the gateway probe fails', async ({ page }) => {
    await page.goto('/');

    const connectButton = page.locator('button', { hasText: 'Initialize Connection' });
    const gatewayInput = page.getByLabel('Gateway URL');

    await expect(gatewayInput).toBeVisible();
    await gatewayInput.fill('http://127.0.0.1:9');
    await connectButton.click();

    await expect(page.getByText(/Could not connect to gateway/)).toBeVisible({ timeout: 15000 });
    await expect(gatewayInput).toBeVisible();
  });

  test('connect reads browser-filled field values that did not fire React change events', async ({ page }) => {
    await page.goto('/');

    const connectButton = page.locator('button', { hasText: 'Initialize Connection' });
    const gatewayInput = page.getByLabel('Gateway URL');
    const tokenInput = page.getByLabel('Authorization Token');

    await expect(gatewayInput).toBeVisible();
    await gatewayInput.evaluate((node) => {
      (node as HTMLInputElement).value = 'http://127.0.0.1:9';
    });
    await tokenInput.evaluate((node) => {
      (node as HTMLInputElement).value = 'autofilled-token';
    });

    await expect(connectButton).toBeEnabled();
    await connectButton.click();

    await expect(page.getByText(/Could not connect to gateway/)).toBeVisible({ timeout: 15000 });
    await expect(gatewayInput).toBeVisible();
  });

  test('connect timeout exits loading state with a visible error', async ({ page }) => {
    await page.route('**/talon.v1.NamespaceService/List', () => {
      // Simulate a gateway request that never resolves.
    });
    await page.goto('/');

    const connectButton = page.locator('button', { hasText: /Initialize Connection|Connecting/ });
    const gatewayInput = page.getByLabel('Gateway URL');
    const tokenInput = page.getByLabel('Authorization Token');

    await expect(gatewayInput).toBeVisible();
    await gatewayInput.fill(new URL(page.url()).origin);
    await tokenInput.fill('valid-looking-token');
    await connectButton.click();

    await expect(connectButton).toContainText('Connecting');
    await expect(page.getByText(/request timed out/)).toBeVisible({ timeout: 12000 });
    await expect(connectButton).toContainText('Initialize Connection');
    await expect(connectButton).toBeEnabled();
  });

  test('should load and connect to the gateway', async ({ page }) => {
    // 1. Visit the page
    await page.goto('/');

    // 2. Connect to the Gateway
    const connectButton = page.locator('button', { hasText: 'Initialize Connection' });
    const gatewayInput = page.getByLabel('Gateway URL');
    const tokenInput = page.getByLabel('Authorization Token');
    
    // Ensure we are in disconnected state showing the form
    await expect(gatewayInput).toBeVisible();
    
    // Use the backend port
    const API_PORT = process.env.API_PORT || '50051';
    await gatewayInput.fill(`http://127.0.0.1:${API_PORT}`);
    await tokenInput.fill(createLocalRootToken());
    await connectButton.click();

    // 3. Ensure we are connected and stay out of the auth fork after URL sync settles.
    await expect(page.locator('text=Connected')).toBeVisible({ timeout: 45000 });
    await expect(page).toHaveURL(/connected=true/);
    await expect(gatewayInput).toBeHidden();
    await page.waitForTimeout(1000);
    await expect(page).toHaveURL(/connected=true/);
    await expect(gatewayInput).toBeHidden();
  });
});
