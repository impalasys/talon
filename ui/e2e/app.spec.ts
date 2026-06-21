import { test, expect } from '@playwright/test';

test.describe('Talon UI', () => {
  test('should load and connect to the gateway', async ({ page }) => {
    // 1. Visit the page
    await page.goto('/');

    // 2. Connect to the Gateway
    const connectButton = page.locator('button', { hasText: 'Initialize Connection' });
    const gatewayInput = page.locator('input[type="url"]');
    
    // Ensure we are in disconnected state showing the form
    await expect(gatewayInput).toBeVisible();
    
    // Use the backend port
    const API_PORT = process.env.API_PORT || '50051';
    await gatewayInput.fill(`http://127.0.0.1:${API_PORT}`);
    await connectButton.click();

    // 3. Ensure we are connected
    await expect(page.locator('text=Connected')).toBeVisible({ timeout: 45000 });
  });
});
