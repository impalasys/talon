import { test, expect } from '@playwright/test';
import { createClient } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { GatewayService } from "../proto/proto/gateway_pb";

async function provisionSession(page: any) {
  page.on('console', msg => console.log(`BROWSER CONSOLE: ${msg.text()}`));
  page.on('pageerror', error => console.log(`BROWSER ERROR: ${error.message}`));

  const API_PORT = process.env.API_PORT || '18789';
  const gatewayUrl = `http://127.0.0.1:${API_PORT}`;
  const runId = Date.now().toString();
  const testNs = `e2e-ns-${runId}`;
  const testAgent = `e2e-agent-${runId}`;

  const client = createClient(GatewayService, createGrpcWebTransport({ baseUrl: gatewayUrl }));

  await expect(async () => {
    await client.createNamespace({ name: testNs, recursive: true });
  }).toPass({ timeout: 60000 });

  await client.createAgent({
    ns: testNs,
    name: testAgent,
    definition: {
      source: {
        case: "customSpec",
        value: {
          modelPolicy: {
            profiles: [
              {
                name: "default",
                model: { provider: "mock", name: "minimax", temperature: 0.7 },
              },
            ],
          },
          systemPrompt: "Stream me",
          mcpServerRefs: []
        }
      }
    }
  });

  const sessionRes = await client.createSession({
    ns: testNs,
    agent: testAgent
  });

  await page.goto('/');
  const connectButton = page.locator('button', { hasText: 'Initialize Connection' });
  const gatewayInput = page.locator('input[type="url"]');
  await expect(gatewayInput).toBeVisible();
  await gatewayInput.fill(gatewayUrl);
  await connectButton.click();
  await expect(page.locator('text=Connected')).toBeVisible({ timeout: 15000 });

  const nsNode = page.locator('.truncate', { hasText: testNs }).first();
  await expect(nsNode).toBeVisible({ timeout: 15000 });
  await nsNode.click();

  const agentNode = page.locator('.truncate', { hasText: testAgent }).first();
  await expect(agentNode).toBeVisible({ timeout: 5000 });
  await agentNode.click();

  const sessionLink = page.locator('.truncate', { hasText: /AM|PM|Mins|Secs/i }).first();
  await expect(sessionLink).toBeVisible({ timeout: 5000 });
  await sessionLink.click();

  const chatInput = page.locator('textarea[placeholder="Ask Talon to perform a task..."]');
  await expect(chatInput).toBeVisible({ timeout: 5000 });

  return { chatInput, sessionId: sessionRes.sessionId, gatewayUrl };
}

test.describe('Chat Streaming', () => {
  test('should send chat messages through the gateway UI transport', async ({ page }) => {
    const { chatInput } = await provisionSession(page);
    await chatInput.click();
    await page.keyboard.type('square root of 144');
    await page.waitForTimeout(1000);
    await chatInput.press('Enter');
    await page.waitForTimeout(3000);

    // 5. Verify the streaming sequence
    await expect(page.getByText('square root of 144', { exact: true })).toBeVisible({ timeout: 5000 });
    await expect(page.getByText('The square root of 144 is 12.', { exact: true })).toBeVisible({ timeout: 30000 });
  });

  test('should render tool calls live without reloading the page', async ({ page }) => {
    const { chatInput } = await provisionSession(page);

    await chatInput.click();
    await page.keyboard.type('lookup talon.impala.systems');
    await chatInput.press('Enter');

    await expect(page.getByText('lookup talon.impala.systems', { exact: true })).toBeVisible({ timeout: 5000 });
    await expect(page.getByRole('button', { name: 'Ran 1 tool' })).toBeVisible({ timeout: 30000 });
    await expect(page.getByText('⏳ Calling knowledge_search')).toBeVisible({ timeout: 10000 });

    await page.getByRole('button', { name: 'Ran 1 tool' }).click();
    await expect(page.getByText('Tool:')).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('knowledge_search', { exact: true })).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('I checked knowledge_search for talon.impala.systems.', { exact: true })).toBeVisible({ timeout: 30000 });
  });
});
