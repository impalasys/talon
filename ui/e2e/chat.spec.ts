import { test, expect } from '@playwright/test';
import { createClient } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { GatewayService } from "../proto/proto/gateway_pb";

test.describe('Chat Streaming', () => {
  test('should display streaming text from the agent', async ({ page }) => {
    page.on('console', msg => console.log(`BROWSER CONSOLE: ${msg.text()}`));
    page.on('pageerror', error => console.log(`BROWSER ERROR: ${error.message}`));
    
    const API_PORT = process.env.API_PORT || '18789';
    const gatewayUrl = `http://127.0.0.1:${API_PORT}`;
    const runId = Date.now().toString();
    const testNs = `e2e-ns-${runId}`;
    const testAgent = `e2e-agent-${runId}`;

    // 1. Provision backend state using gRPC-Web directly
    // This makes the test fast and robust by avoiding brittle context-menu UI interactions
    const client = createClient(GatewayService, createGrpcWebTransport({ baseUrl: gatewayUrl }));

    await expect(async () => {
      // Retry creating the namespace until the backend has fully started
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
    const sessionId = sessionRes.sessionId;

    // 2. Visit the page and Connect
    await page.goto('/');
    const connectButton = page.locator('button', { hasText: 'Initialize Connection' });
    const gatewayInput = page.locator('input[type="url"]');
    await expect(gatewayInput).toBeVisible();
    await gatewayInput.fill(gatewayUrl);
    await connectButton.click();
    await expect(page.locator('text=Connected')).toBeVisible({ timeout: 15000 });

    // 3. Navigate to the Session in the Sidebar
    const nsNode = page.locator('.truncate', { hasText: testNs }).first();
    await expect(nsNode).toBeVisible({ timeout: 15000 });
    await nsNode.click(); // Expand Namespace

    const agentNode = page.locator('.truncate', { hasText: testAgent }).first();
    await expect(agentNode).toBeVisible({ timeout: 5000 });
    await agentNode.click(); // Expand Agent

    // Click on the specific session we created
    const sessionLink = page.locator('.truncate', { hasText: /AM|PM|Mins|Secs/i }).first();
    await expect(sessionLink).toBeVisible({ timeout: 5000 });
    await sessionLink.click();

    // Now we should be in a session chat
    await expect(page.locator('text=Talon runtime initialized.')).toBeVisible({ timeout: 5000 });

    // 4. Send a message that triggers the mock LLM streaming response
    const chatInput = page.locator('textarea[placeholder="Ask Talon to perform a task..."]');
    await chatInput.click();
    await page.keyboard.type('square root of 144');
    await page.waitForTimeout(1000);
    await chatInput.press('Enter');

    // 5. Verify the streaming sequence
    // Check for the Thinking status first
    await expect(page.locator('text=⏳ Thinking...')).toBeVisible({ timeout: 10000 });
    
    // Wait for the final text from the stream
    await expect(page.locator('text=The square root of 144 is 12.')).toBeVisible({ timeout: 15000 });
  });
});
