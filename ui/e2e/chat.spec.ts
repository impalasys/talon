import { test, expect } from '@playwright/test';
import fs from 'node:fs/promises';
import path from 'node:path';
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
  const sendButton = page.locator('form').filter({ has: chatInput }).getByRole('button');
  await expect(chatInput).toBeVisible({ timeout: 5000 });

  return { chatInput, sendButton, sessionId: sessionRes.sessionId, gatewayUrl, client, testNs, testAgent };
}

test.describe('Chat Streaming', () => {
  test('should send chat messages through the gateway UI transport', async ({ page }) => {
    const { chatInput, sendButton } = await provisionSession(page);
    await chatInput.click();
    await chatInput.fill('square root of 144');
    await expect(sendButton).toBeEnabled({ timeout: 5000 });
    await sendButton.click();

    // 5. Verify the streaming sequence
    await expect(page.getByText('square root of 144', { exact: true })).toBeVisible({ timeout: 5000 });
    await expect(page.getByText('The square root of 144 is 12.', { exact: true })).toBeVisible({ timeout: 30000 });
  });

  test('should render and replay thinking blocks from the mock llm', async ({ page }) => {
    const { client, sessionId, testNs, testAgent } = await provisionSession(page);
    await client.sendMessage({
      ns: testNs,
      agent: testAgent,
      sessionId,
      message: 'hello',
      labels: {},
    });

    await expect(async () => {
      const session = await client.getSession({
        ns: testNs,
        agent: testAgent,
        sessionId,
      }) as any;
      expect(session.messages?.some((message: any) => message.content === 'Hello! I am a mock LLM. How can I assist you today?')).toBeTruthy();
      expect((session.steps ?? []).some((step: any) => step.stepType === 6 || step.stepType === 'STEP_TYPE_REASONING')).toBeTruthy();
    }).toPass({ timeout: 30000 });

    await page.reload();
    await expect(page.getByText('Hello! I am a mock LLM. How can I assist you today?', { exact: true })).toBeVisible({ timeout: 30000 });

    const thinkingToggle = page.getByRole('button', { name: /Thinking/ }).last();
    await expect(thinkingToggle).toBeVisible({ timeout: 30000 });
    await expect(thinkingToggle).toContainText('6 reasoning');
    if (process.env.CAPTURE_THINKING_UI === 'true') {
      const outputDir = path.resolve(__dirname, '../../docs/pr');
      await fs.mkdir(outputDir, { recursive: true });
      await page.screenshot({ path: path.join(outputDir, 'thinking-collapsed.png'), fullPage: true });
    }

    await thinkingToggle.click();
    await expect(page.getByText('Inspecting the request.', { exact: false })).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('Planning a concise answer.', { exact: false })).toBeVisible({ timeout: 10000 });
    if (process.env.CAPTURE_THINKING_UI === 'true') {
      const outputDir = path.resolve(__dirname, '../../docs/pr');
      await page.screenshot({ path: path.join(outputDir, 'thinking-expanded.png'), fullPage: true });
    }

    await page.reload();
    const replayedThinkingToggle = page.getByRole('button', { name: /Thinking/ }).last();
    await expect(replayedThinkingToggle).toBeVisible({ timeout: 30000 });
    await replayedThinkingToggle.click();
    await expect(page.getByText('Inspecting the request.', { exact: false })).toBeVisible({ timeout: 10000 });
  });

  test('should render tool calls interleaved with the answer live', async ({ page }) => {
    const { chatInput, sendButton } = await provisionSession(page);

    await chatInput.click();
    await chatInput.fill('lookup docs.example.com');
    await expect(sendButton).toBeEnabled({ timeout: 5000 });
    await sendButton.click();

    await expect(page.getByText('lookup docs.example.com', { exact: true })).toBeVisible({ timeout: 5000 });
    await expect(page.getByText('Let me check that.', { exact: false })).toBeVisible({ timeout: 30000 });
    await expect(page.getByText('Tool', { exact: true })).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('knowledge_search', { exact: true })).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('I checked knowledge_search for docs.example.com.', { exact: true })).toBeVisible({ timeout: 30000 });

    const transcript = (await page.locator('body').textContent()) || '';
    expect(transcript.indexOf('Let me check that.')).toBeGreaterThan(-1);
    expect(transcript.indexOf('knowledge_search')).toBeGreaterThan(transcript.indexOf('Let me check that.'));
    expect(transcript.indexOf('I checked knowledge_search for docs.example.com.')).toBeGreaterThan(
      transcript.indexOf('knowledge_search'),
    );
  });
});
