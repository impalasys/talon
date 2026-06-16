import { test, expect, type Page } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';
import { createClient } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { GatewayService } from "../proto/proto/gateway_pb";

async function createTestSession() {
  const API_PORT = process.env.API_PORT || '18789';
  const gatewayUrl = `http://127.0.0.1:${API_PORT}`;
  const runId = `${Date.now()}-${randomUUID().slice(0, 8)}`;
  const testNs = `e2e-ns-${runId}`;
  const testAgent = `e2e-agent-${runId}`;

  const client = createClient(GatewayService, createGrpcWebTransport({ baseUrl: gatewayUrl }));

  await expect(async () => {
    await client.createNamespace({ name: testNs, recursive: true });
  }).toPass({ timeout: 60000 });

  await client.createResource({
    ns: testNs,
    resource: {
      apiVersion: "talon.impalasys.com/v1",
      kind: "Agent",
      metadata: { name: testAgent, namespace: testNs, labels: {}, annotations: {}, ownerReferences: [], finalizers: [], generation: BigInt(0), resourceVersion: "", uid: "" },
      spec: {
        kind: {
          case: "agent",
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
            mcpServerRefs: [],
          },
        },
      },
      status: { kind: { case: "agent", value: { observedGeneration: BigInt(0), phase: "", conditions: [] } } },
    },
  });

  const sessionRes = await client.createSession({
    ns: testNs,
    agent: testAgent
  });

  return { sessionId: sessionRes.sessionId, gatewayUrl, client, testNs, testAgent };
}

async function provisionSession(page: Page) {
  page.on('console', msg => console.log(`BROWSER CONSOLE: ${msg.text()}`));
  page.on('pageerror', error => console.log(`BROWSER ERROR: ${error.message}`));

  const { sessionId, gatewayUrl, client, testNs, testAgent } = await createTestSession();

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

  return { chatInput, sendButton, sessionId, gatewayUrl, client, testNs, testAgent };
}

async function waitForSessionText(
  client: any,
  target: { ns: string; agent: string; sessionId: string },
  expectedText: string,
) {
  await expect(async () => {
    const history = await client.listSessionMessages({
      ...target,
      pageSize: 50,
    });
    const contents = (history.items ?? [])
      .map((item: any) => sessionMessageText(item.message))
      .filter(Boolean);
    expect(contents).toContain(expectedText);
  }).toPass({ timeout: 60000 });
}

function sessionMessageText(message: any): string {
  if (Array.isArray(message?.parts)) {
    const content = message.parts
      .filter((part: any) => {
        const type = part?.partType ?? part?.part_type ?? part?.type;
        return type === 1 || type === 'SESSION_MESSAGE_PART_TYPE_TEXT' || type === 'text' || type === 6 || type === 'SESSION_MESSAGE_PART_TYPE_ERROR';
      })
      .map((part: any) => typeof part?.content === 'string' ? part.content : typeof part?.text === 'string' ? part.text : '')
      .join('');
    if (content) return content;
  }
  return typeof message?.content === 'string' ? message.content : '';
}

function hasReasoningPart(message: any): boolean {
  return Array.isArray(message?.parts) && message.parts.some((part: any) => {
    const type = part?.partType ?? part?.part_type ?? part?.type;
    const content = typeof part?.content === 'string' ? part.content : typeof part?.text === 'string' ? part.text : '';
    return content.length > 0 && (type === 2 || type === 'SESSION_MESSAGE_PART_TYPE_REASONING' || type === 'reasoning');
  });
}

async function annotatePaginationProof(page: Page, label: string) {
  if (process.env.CAPTURE_PAGINATION_VIDEO !== 'true') return;
  await page.evaluate((text) => {
    const id = 'pagination-proof-overlay';
    let overlay = document.getElementById(id);
    if (!overlay) {
      overlay = document.createElement('div');
      overlay.id = id;
      Object.assign(overlay.style, {
        position: 'fixed',
        left: '16px',
        bottom: '16px',
        zIndex: '2147483647',
        maxWidth: '760px',
        padding: '12px 14px',
        borderRadius: '12px',
        background: 'rgba(15, 23, 42, 0.94)',
        color: 'white',
        font: '600 16px/1.35 ui-sans-serif, system-ui, sans-serif',
        boxShadow: '0 18px 45px rgba(15, 23, 42, 0.28)',
        whiteSpace: 'pre-line',
      });
      document.body.appendChild(overlay);
    }
    overlay.textContent = text;
  }, label);
  await page.waitForTimeout(2200);
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

  test('should show the slash command menu and clear the active session', async ({ page }) => {
    const { chatInput, sendButton, client, sessionId, testNs, testAgent } = await provisionSession(page);
    const target = { ns: testNs, agent: testAgent, sessionId };

    await chatInput.click();
    await chatInput.fill('square root of 144');
    await expect(sendButton).toBeEnabled({ timeout: 5000 });
    await sendButton.click();

    await expect(page.getByText('square root of 144', { exact: true })).toBeVisible({ timeout: 5000 });
    await expect(page.getByText('The square root of 144 is 12.', { exact: true })).toBeVisible({ timeout: 30000 });
    await waitForSessionText(client, target, 'square root of 144');

    await chatInput.fill('/');
    const commandMenu = page.getByRole('listbox', { name: 'Command menu' });
    await expect(commandMenu).toBeVisible({ timeout: 5000 });

    const clearOption = page.getByRole('option', { name: /\/clear/i });
    await expect(clearOption).toBeVisible();
    await clearOption.hover();
    await expect(clearOption).toHaveCSS('background-color', 'rgba(24, 24, 27, 0.11)');
    await clearOption.click();

    await expect(chatInput).toHaveValue('/clear');
    await expect(sendButton).toBeEnabled({ timeout: 5000 });
    await sendButton.click();

    await expect(page.getByText('square root of 144', { exact: true })).toHaveCount(0, { timeout: 10000 });
    await expect(page.getByText('The square root of 144 is 12.', { exact: true })).toHaveCount(0);
    await expect(async () => {
      const history = await client.listSessionMessages({
        ...target,
        pageSize: 50,
      });
      expect(history.items ?? []).toHaveLength(0);
    }).toPass({ timeout: 30000 });
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
      const history = await client.listSessionMessages({
        ns: testNs,
        agent: testAgent,
        sessionId,
        pageSize: 50,
      }) as any;
      expect((history.items ?? []).some((item: any) => sessionMessageText(item.message) === 'Hello! I am a mock LLM. How can I assist you today?')).toBeTruthy();
      expect((history.items ?? []).some((item: any) => hasReasoningPart(item.message))).toBeTruthy();
    }).toPass({ timeout: 30000 });

    await page.reload();
    await expect(page.getByText('Hello! I am a mock LLM. How can I assist you today?', { exact: true })).toBeVisible({ timeout: 30000 });

    const workToggle = page.getByRole('button', { name: /Worked for \d+s/ }).last();
    await expect(workToggle).toBeVisible({ timeout: 30000 });
    if (process.env.CAPTURE_THINKING_UI === 'true') {
      const outputDir = path.resolve(__dirname, '../../docs/pr');
      await fs.mkdir(outputDir, { recursive: true });
      await page.screenshot({ path: path.join(outputDir, 'thinking-collapsed.png'), fullPage: true });
    }

    await workToggle.click();
    await expect(page.getByText('Inspecting the request.', { exact: false })).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('Planning a concise answer.', { exact: false })).toBeVisible({ timeout: 10000 });
    if (process.env.CAPTURE_THINKING_UI === 'true') {
      const outputDir = path.resolve(__dirname, '../../docs/pr');
      await page.screenshot({ path: path.join(outputDir, 'thinking-expanded.png'), fullPage: true });
    }

    await page.reload();
    const replayedWorkToggle = page.getByRole('button', { name: /Worked for \d+s/ }).last();
    await expect(replayedWorkToggle).toBeVisible({ timeout: 30000 });
    await replayedWorkToggle.click();
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
    await expect(page.getByText('I checked knowledge_search for docs.example.com.', { exact: true })).toBeVisible({ timeout: 30000 });

    const workToggle = page.getByRole('button', { name: /Worked for \d+s/ }).last();
    await expect(workToggle).toBeVisible({ timeout: 10000 });
    await workToggle.click();
    await expect(page.getByText(/Called\s+knowledge_search/)).toBeVisible({ timeout: 10000 });

    const transcript = (await page.locator('body').textContent()) || '';
    expect(transcript).toContain('Let me check that.');
    expect(transcript).toContain('Called knowledge_search');
    expect(transcript).toContain('I checked knowledge_search for docs.example.com.');
  });
});

test.describe('Copilot history pagination', () => {
  test('loads older session message pages on transcript scroll without fetching full history', async ({ browser }, testInfo) => {
    const { client, sessionId, gatewayUrl, testNs, testAgent } = await createTestSession();
    const target = { ns: testNs, agent: testAgent, sessionId };

    for (let index = 1; index <= 5; index += 1) {
      const prompt = `pagination seed ${index}`;
      await client.sendMessage({
        ...target,
        message: prompt,
        labels: {},
      });
      await waitForSessionText(client, target, `I received your message: ${prompt}`);
    }

    const webPort = process.env.WEB_PORT || '3000';
    const context = await browser.newContext({
      baseURL: `http://localhost:${webPort}`,
      viewport: { width: 1280, height: 720 },
      recordVideo: process.env.CAPTURE_E2E_VIDEO === 'true'
        ? { dir: testInfo.outputDir, size: { width: 1280, height: 720 } }
        : undefined,
    });
    const page = await context.newPage();
    const listSessionMessagesRequests: string[] = [];
    const getSessionRequests: string[] = [];
    page.on('request', request => {
      const url = request.url();
      if (url.includes('/talon.gateway.GatewayService/ListSessionMessages')) {
        listSessionMessagesRequests.push(url);
      }
      if (url.includes('/talon.gateway.GatewayService/GetSession')) {
        getSessionRequests.push(url);
      }
    });

    try {
      await page.addInitScript((url) => {
        localStorage.setItem('talon_gateway_url', url);
      }, gatewayUrl);

      await page.goto(`/?connected=true&historyPageSize=4&ns=${encodeURIComponent(testNs)}&agent=${encodeURIComponent(testAgent)}&session=${encodeURIComponent(sessionId)}`);
      await expect(page.locator('text=Connected')).toBeVisible({ timeout: 45000 });
      await expect(page.getByText('pagination seed 5', { exact: true })).toBeVisible({ timeout: 30000 });
      await expect(page.getByText('I received your message: pagination seed 5', { exact: true })).toBeVisible({ timeout: 30000 });
      await expect(page.getByText('pagination seed 1', { exact: true })).toHaveCount(0);
      await expect.poll(() => listSessionMessagesRequests.length).toBeGreaterThanOrEqual(1);
      expect(getSessionRequests).toHaveLength(0);
      await annotatePaginationProof(
        page,
        `Initial page loaded\nVisible: seed 5 newest page\nAbsent: seed 1 older page\nListSessionMessages calls: ${listSessionMessagesRequests.length}\nGetSession calls: ${getSessionRequests.length}`,
      );

      const transcript = page.getByTestId('copilot-transcript');
      await expect(transcript).toBeVisible();

      await transcript.evaluate((element) => {
        element.scrollTop = 0;
        element.dispatchEvent(new Event('scroll', { bubbles: true }));
      });
      await expect(page.getByText('pagination seed 3', { exact: true })).toBeVisible({ timeout: 30000 });
      await expect.poll(() => listSessionMessagesRequests.length).toBeGreaterThanOrEqual(2);
      expect(getSessionRequests).toHaveLength(0);
      await annotatePaginationProof(
        page,
        `After first scroll-to-top\nVisible: seed 3 from older page\nListSessionMessages calls: ${listSessionMessagesRequests.length}\nGetSession calls: ${getSessionRequests.length}`,
      );

      await transcript.evaluate((element) => {
        element.scrollTop = 0;
        element.dispatchEvent(new Event('scroll', { bubbles: true }));
      });
      await expect(page.getByText('pagination seed 1', { exact: true })).toBeVisible({ timeout: 30000 });
      await expect.poll(() => listSessionMessagesRequests.length).toBeGreaterThanOrEqual(3);
      expect(getSessionRequests).toHaveLength(0);
      await annotatePaginationProof(
        page,
        `After second scroll-to-top\nVisible: seed 1 oldest page\nListSessionMessages calls: ${listSessionMessagesRequests.length}\nGetSession calls: ${getSessionRequests.length}`,
      );
    } finally {
      await context.close();
    }
  });
});
