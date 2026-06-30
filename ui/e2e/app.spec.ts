import { test, expect } from '@playwright/test';
import { installBrowserAuth, e2eGatewayUrl, readE2EAuth } from './talonAuth';

test.describe('Talon UI', () => {
  test('should load and connect to the gateway', async ({ page }) => {
    const gatewayUrl = e2eGatewayUrl();
    await installBrowserAuth(page, gatewayUrl);

    // 1. Visit the page
    await page.goto('/');

    // 2. Connect to the Gateway
    const connectButton = page.locator('button', { hasText: 'Initialize Connection' });
    const gatewayInput = page.locator('input[type="url"]');
    
    // Ensure we are in disconnected state showing the form
    await expect(gatewayInput).toBeVisible();
    
    await gatewayInput.fill(gatewayUrl);
    const auth = readE2EAuth();
    if (auth?.accessToken) {
      await page.locator('input[type="password"]').fill(auth.accessToken);
    }
    await connectButton.click();

    // 3. Ensure we are connected
    await expect(page.locator('text=Connected')).toBeVisible({ timeout: 45000 });
  });
});
