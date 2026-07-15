import { test, expect, type Page } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';
import { decompress as decompressZstd } from 'fzstd';
import { createE2ETalonClient, e2eGatewayUrl, installBrowserAuth } from './talonAuth';

async function createTestSession(options: { mcpServerRefs?: string[] } = {}) {
  const gatewayUrl = e2eGatewayUrl();
  const runId = `${Date.now()}-${randomUUID().slice(0, 8)}`;
  const testNs = `e2e-ns-${runId}`;
  const testAgent = `e2e-agent-${runId}`;

  const client = createE2ETalonClient(gatewayUrl);

  await expect(async () => {
    await client.namespaces.create({ name: testNs, recursive: true });
  }).toPass({ timeout: 60000 });

  await client.resources.create({
    ns: testNs,
    manifest: {
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
            mcpServerRefs: options.mcpServerRefs ?? [],
          },
        },
      },
    },
  });

  const sessionRes = await client.sessions.create({
    ns: testNs,
    agent: testAgent
  });

  return { sessionId: sessionRes.sessionId, gatewayUrl, client, testNs, testAgent };
}

async function createMcpTestSession() {
  const mcpServer = 'durable-slow';
  const session = await createTestSession({ mcpServerRefs: [mcpServer] });
  const mockLlmPort = process.env.MOCK_LLM_PORT || '8000';

  await session.client.resources.create({
    ns: session.testNs,
    manifest: {
      apiVersion: "talon.impalasys.com/v1",
      kind: "McpServer",
      metadata: { name: mcpServer, namespace: session.testNs, labels: {}, annotations: {}, ownerReferences: [], finalizers: [], generation: BigInt(0), resourceVersion: "", uid: "" },
      spec: {
        kind: {
          case: "mcpServer",
          value: {
            transport: "http",
            target: `http://127.0.0.1:${mockLlmPort}/mcp`,
            args: [],
            headers: {},
            disabled: false,
          },
        },
      },
    },
  });

  return session;
}

async function provisionSession(page: Page) {
  page.on('console', msg => console.log(`BROWSER CONSOLE: ${msg.text()}`));
  page.on('pageerror', error => console.log(`BROWSER ERROR: ${error.message}`));

  const { sessionId, gatewayUrl, client, testNs, testAgent } = await createTestSession();

  await installBrowserAuth(page, gatewayUrl);
  await page.goto('/?connected=true');

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
  const sendButton = page.locator('form').filter({ has: chatInput }).getByRole('button', { name: 'Send message' });
  await expect(chatInput).toBeVisible({ timeout: 5000 });

  return { chatInput, sendButton, sessionId, gatewayUrl, client, testNs, testAgent };
}

async function provisionMcpSession(page: Page) {
  page.on('console', msg => console.log(`BROWSER CONSOLE: ${msg.text()}`));
  page.on('pageerror', error => console.log(`BROWSER ERROR: ${error.message}`));

  const { sessionId, gatewayUrl, client, testNs, testAgent } = await createMcpTestSession();

  await installBrowserAuth(page, gatewayUrl);
  await page.goto('/?connected=true');

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
  const sendButton = page.locator('form').filter({ has: chatInput }).getByRole('button', { name: 'Send message' });
  await expect(chatInput).toBeVisible({ timeout: 5000 });

  return { chatInput, sendButton, sessionId, gatewayUrl, client, testNs, testAgent };
}

async function decodeCasText(response: any, data: Uint8Array): Promise<string> {
  const encoding = String(response?.contentEncoding || response?.metadata?.contentEncoding || '').toLowerCase();
  if (encoding === 'zstd') {
    return new TextDecoder().decode(decompressZstd(data));
  }
  if (encoding === 'gzip') {
    const stream = new Blob([data as unknown as BlobPart]).stream().pipeThrough(new DecompressionStream('gzip'));
    return await new Response(stream).text();
  }
  return new TextDecoder().decode(data);
}

async function waitForSessionText(
  client: any,
  target: { ns: string; agent: string; sessionId: string },
  expectedText: string,
) {
  await expect(async () => {
    const history = await client.sessions.listMessages({
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

function sessionMessageProjectionState(message: any): string {
  return message?.labels?.['talon.session.projection_state'] ?? '';
}

async function waitForCommittedSessionText(
  client: any,
  target: { ns: string; agent: string; sessionId: string },
  expectedText: string,
) {
  await expect(async () => {
    const history = await client.sessions.listMessages({
      ...target,
      pageSize: 50,
    });
    const match = (history.items ?? [])
      .map((item: any) => item.message)
      .find((message: any) => sessionMessageText(message) === expectedText);
    expect(match).toBeTruthy();
    expect(sessionMessageProjectionState(match)).toBe('committed');
    expect(history.state).toBe('IDLE');
  }).toPass({ timeout: 60000 });
}

function hasReasoningPart(message: any): boolean {
  return Array.isArray(message?.parts) && message.parts.some((part: any) => {
    const type = part?.partType ?? part?.part_type ?? part?.type;
    const content = typeof part?.content === 'string' ? part.content : typeof part?.text === 'string' ? part.text : '';
    return content.length > 0 && (type === 2 || type === 'SESSION_MESSAGE_PART_TYPE_REASONING' || type === 'reasoning');
  });
}

function sessionPartType(part: any): unknown {
  return part?.partType ?? part?.part_type ?? part?.type;
}

function sessionPartContent(part: any): string {
  return typeof part?.content === 'string' ? part.content : typeof part?.text === 'string' ? part.text : '';
}

function sessionPartsOfType(message: any, expectedType: 'text' | 'reasoning' | 'usage' | 'toolResult') {
  const typeValues = {
    text: new Set([1, 'SESSION_MESSAGE_PART_TYPE_TEXT', 'text']),
    reasoning: new Set([2, 'SESSION_MESSAGE_PART_TYPE_REASONING', 'reasoning']),
    usage: new Set([5, 'SESSION_MESSAGE_PART_TYPE_USAGE', 'usage']),
    toolResult: new Set([4, 'SESSION_MESSAGE_PART_TYPE_TOOL_RESULT', 'tool_result']),
  }[expectedType];
  return Array.isArray(message?.parts)
    ? message.parts.filter((part: any) => typeValues.has(sessionPartType(part) as any))
    : [];
}

function sessionPartPayload(part: any): Record<string, any> {
  const payload = part?.payloadJson ?? part?.payload_json ?? '';
  return typeof payload === 'string' && payload.length > 0 ? JSON.parse(payload) : {};
}

async function rootCssVar(page: Page, name: string) {
  return page.evaluate((variableName) => {
    return getComputedStyle(document.documentElement).getPropertyValue(variableName).trim();
  }, name);
}

function cssVarPattern(...values: string[]) {
  return new RegExp(`^(?:${values.map((value) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|')})$`);
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

async function screenshotOutputPath(testInfo: { outputPath: (...pathSegments: string[]) => string }, filename: string) {
  const configuredDir = process.env.SIGHTLINE_SCREENSHOT_DIR;
  const outputDir = configuredDir
    ? path.resolve(configuredDir)
    : testInfo.outputPath('sightline-screenshots');
  await fs.mkdir(outputDir, { recursive: true });
  return path.join(outputDir, filename);
}

test.describe('Sightline theme tokens', () => {
  test('resolves the browser light preference by default', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await page.goto('/');

    await expect.poll(async () => {
      return page.evaluate(() => ({
        className: document.documentElement.className,
        titleColor: getComputedStyle(document.documentElement).getPropertyValue('--color-title-50').trim(),
        inputBg: getComputedStyle(document.documentElement).getPropertyValue('--copilot-input-bg').trim(),
        expandedInputBg: getComputedStyle(document.documentElement).getPropertyValue('--copilot-input-expanded-bg').trim(),
        bubbleFg: getComputedStyle(document.documentElement).getPropertyValue('--talon-chat-user-bubble-fg').trim(),
      }));
    }).toMatchObject({
      className: expect.stringContaining('light'),
      titleColor: '#0f172a',
      inputBg: expect.stringMatching(cssVarPattern('rgba(255, 255, 255, 0.96)', '#fffffff5')),
      expandedInputBg: expect.stringMatching(cssVarPattern('var(--copilot-input-bg)', 'rgba(255, 255, 255, 0.96)', '#fffffff5')),
      bubbleFg: '#0f172a',
    });
  });

  test('keeps app and chat tokens aligned for explicit light and dark classes', async ({ page }) => {
    await page.goto('/');

    await page.evaluate(() => {
      document.documentElement.classList.remove('dark');
      document.documentElement.classList.add('light');
    });
    await expect.poll(() => rootCssVar(page, '--color-title-50')).toBe('#0f172a');
    await expect.poll(() => rootCssVar(page, '--copilot-input-bg')).toMatch(cssVarPattern('rgba(255, 255, 255, 0.96)', '#fffffff5'));
    await expect.poll(() => rootCssVar(page, '--copilot-input-expanded-bg')).toMatch(cssVarPattern('var(--copilot-input-bg)', 'rgba(255, 255, 255, 0.96)', '#fffffff5'));
    await expect.poll(() => rootCssVar(page, '--talon-chat-user-bubble-fg')).toBe('#0f172a');

    await page.evaluate(() => {
      document.documentElement.classList.remove('light');
      document.documentElement.classList.add('dark');
    });
    await expect.poll(() => rootCssVar(page, '--color-title-50')).toBe('#f4f7ff');
    await expect.poll(() => rootCssVar(page, '--copilot-input-bg')).toMatch(cssVarPattern('rgba(15, 23, 42, 0.92)', '#0f172aeb'));
    await expect.poll(() => rootCssVar(page, '--copilot-input-expanded-bg')).toMatch(cssVarPattern('var(--copilot-input-bg)', 'rgba(15, 23, 42, 0.92)', '#0f172aeb'));
    await expect.poll(() => rootCssVar(page, '--talon-chat-user-bubble-fg')).toBe('#e6edf3');
  });
});

test.describe('Sightline screenshots', () => {
  test('captures the connected chat surface in light and dark mode @screenshots', async ({ page }, testInfo) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    const { chatInput } = await provisionSession(page);
    await chatInput.fill('Sightline screenshot smoke');

    const composer = page.locator('form').filter({ has: chatInput });
    await expect(composer).toBeVisible({ timeout: 5000 });
    await expect(composer).toHaveCSS('background-color', cssVarPattern('rgba(15, 23, 42, 0.92)', 'rgba(15, 23, 42, 0.9)'));
    await page.screenshot({
      path: await screenshotOutputPath(testInfo, 'sightline-chat-dark.png'),
      fullPage: true,
    });

    await page.emulateMedia({ colorScheme: 'light' });
    await expect(page.locator('html')).toHaveClass(/light/);
    await expect(composer).toHaveCSS('background-color', cssVarPattern('rgba(255, 255, 255, 0.96)', 'rgb(255, 255, 255)'));
    await page.screenshot({
      path: await screenshotOutputPath(testInfo, 'sightline-chat-light.png'),
      fullPage: true,
    });
  });
});

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
    await expect.poll(async () => clearOption.evaluate((element) => getComputedStyle(element).backgroundColor))
      .toMatch(cssVarPattern('rgba(148, 163, 184, 0.16)', 'rgba(15, 23, 42, 0.06)'));
    await clearOption.click();

    await expect(chatInput).toHaveValue('/clear');
    await expect(sendButton).toBeEnabled({ timeout: 5000 });
    await sendButton.click();

    await expect(page.getByText('square root of 144', { exact: true })).toHaveCount(0, { timeout: 10000 });
    await expect(page.getByText('The square root of 144 is 12.', { exact: true })).toHaveCount(0);
    await expect(async () => {
      const history = await client.sessions.listMessages({
        ...target,
        pageSize: 50,
      });
      expect(history.items ?? []).toHaveLength(0);
    }).toPass({ timeout: 30000 });
  });

  test('should render and replay thinking blocks from the mock llm', async ({ page }) => {
    const { client, sessionId, testNs, testAgent } = await provisionSession(page);
    await client.sessions.sendMessage({
      ns: testNs,
      agent: testAgent,
      sessionId,
      message: 'hello',
      labels: {},
    });

    await expect(async () => {
      const history = await client.sessions.listMessages({
        ns: testNs,
        agent: testAgent,
        sessionId,
        pageSize: 50,
      }) as any;
      const message = (history.items ?? [])
        .map((item: any) => item.message)
        .find((candidate: any) => sessionMessageText(candidate) === 'Hello! I am a mock LLM. How can I assist you today?');
      expect(message).toBeTruthy();
      expect(hasReasoningPart(message)).toBeTruthy();
      expect(sessionMessageProjectionState(message)).toBe('committed');
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

  test('should persist streamed reasoning and text as coarse session message parts', async ({ page }) => {
    const { client, sessionId, testNs, testAgent } = await provisionSession(page);
    const target = { ns: testNs, agent: testAgent, sessionId };
    const expectedText = 'Hello! I am a mock LLM. How can I assist you today?';
    const expectedReasoning = 'Inspecting the request. Planning a concise answer. ';

    await client.sessions.sendMessage({
      ...target,
      message: 'hello',
      labels: {},
    });

    await expect(async () => {
      const history = await client.sessions.listMessages({
        ...target,
        pageSize: 50,
      }) as any;
      const message = (history.items ?? [])
        .map((item: any) => item.message)
        .find((candidate: any) => sessionMessageText(candidate) === expectedText);

      expect(message).toBeTruthy();
      expect(sessionMessageProjectionState(message)).toBe('committed');
      expect(history.state).toBe('IDLE');

      const reasoningParts = sessionPartsOfType(message, 'reasoning');
      const textParts = sessionPartsOfType(message, 'text');
      const usageParts = sessionPartsOfType(message, 'usage');

      expect(reasoningParts.map(sessionPartContent).join('')).toBe(expectedReasoning);
      expect(reasoningParts).toHaveLength(1);
      expect(textParts.map(sessionPartContent).join('')).toBe(expectedText);
      expect(textParts).toHaveLength(1);
      expect(usageParts).toHaveLength(1);
      expect(message.parts.indexOf(reasoningParts[0])).toBeLessThan(message.parts.indexOf(textParts[0]));
      expect(message.parts.map(sessionPartType).map(String)).toEqual(expect.arrayContaining([
        expect.stringMatching(/REASONING|^2$/),
        expect.stringMatching(/TEXT|^1$/),
        expect.stringMatching(/USAGE|^5$/),
      ]));
    }).toPass({ timeout: 30000 });
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

  test('should hydrate object-backed tool results through CAS on replay', async ({ page }) => {
    const { chatInput, sendButton, client, sessionId, testNs, testAgent } = await provisionMcpSession(page);
    const target = { ns: testNs, agent: testAgent, sessionId };

    await chatInput.click();
    await chatInput.fill('Please run a blocking lookup docs.example.com and summarize what you found.');
    await expect(sendButton).toBeEnabled({ timeout: 5000 });
    await sendButton.click();

    await expect(page.getByText('I checked blocking_lookup for docs.example.com.', { exact: true })).toBeVisible({ timeout: 30000 });

    let toolResultPart: any;
    await expect(async () => {
      const history = await client.sessions.listMessages({
        ...target,
        pageSize: 50,
      }) as any;
      const message = (history.items ?? [])
        .map((item: any) => item.message)
        .find((candidate: any) => sessionMessageText(candidate).includes('I checked blocking_lookup for docs.example.com.'));

      expect(message).toBeTruthy();
      expect(sessionMessageProjectionState(message)).toBe('committed');

      const toolResults = sessionPartsOfType(message, 'toolResult');
      expect(toolResults).toHaveLength(1);
      toolResultPart = toolResults[0];
      expect(sessionPartContent(toolResultPart)).toBe('');
      expect(toolResultPart.object?.key).toMatch(new RegExp(`^cas/${testNs}/sessions/${sessionId}/messages/`));

      const payload = sessionPartPayload(toolResultPart);
      expect(payload.output).toBeUndefined();
      expect(payload.output_preview).toBeUndefined();
      expect(payload.output_object_key).toBe(toolResultPart.object.key);
    }).toPass({ timeout: 30000 });

    const fetched = await client.cas.getObject({ key: toolResultPart.object.key });
    const fetchedBytes = fetched.signedUrl
      ? new Uint8Array(await (await fetch(fetched.signedUrl)).arrayBuffer())
      : fetched.data;
    const hydrated = await decodeCasText(fetched, fetchedBytes);
    expect(hydrated).toContain('blocking_lookup result for docs.example.com');
    expect(hydrated).toContain('reference section 079');

    await page.reload();
    const workToggle = page.getByRole('button', { name: /Worked for \d+s/ }).last();
    await expect(workToggle).toBeVisible({ timeout: 30000 });
    await workToggle.click();
    const toolToggle = page.getByRole('button', { name: /Called\s+mcp_durable_slow_blocking_lookup/ }).last();
    await expect(toolToggle).toBeVisible({ timeout: 10000 });
    await toolToggle.click();
    await expect(page.locator('code').filter({ hasText: 'reference section 079' }).last()).toBeVisible({ timeout: 10000 });
  });
});

test.describe('Copilot history pagination', () => {
  test('loads older session message pages on transcript scroll without fetching full history', async ({ browser }, testInfo) => {
    const { client, sessionId, gatewayUrl, testNs, testAgent } = await createTestSession();
    const target = { ns: testNs, agent: testAgent, sessionId };

    for (let index = 1; index <= 5; index += 1) {
      const prompt = `pagination seed ${index}`;
      await client.sessions.sendMessage({
        ...target,
        message: prompt,
        labels: {},
      });
      await waitForCommittedSessionText(client, target, `I received your message: ${prompt}`);
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
      if (url.includes('/talon.v1.SessionService/ListMessages')) {
        listSessionMessagesRequests.push(url);
      }
      if (url.includes('/talon.v1.SessionService/Get')) {
        getSessionRequests.push(url);
      }
    });

    try {
      await installBrowserAuth(page, gatewayUrl);

      await page.goto(`/?connected=true&historyPageSize=4&ns=${encodeURIComponent(testNs)}&agent=${encodeURIComponent(testAgent)}&session=${encodeURIComponent(sessionId)}`);
      await expect(page.locator('text=Connected')).toBeVisible({ timeout: 45000 });
      await expect(page.getByText('pagination seed 5', { exact: true })).toBeVisible({ timeout: 30000 });
      await expect(page.getByText('I received your message: pagination seed 5', { exact: true })).toBeVisible({ timeout: 30000 });
      await expect(page.getByText('pagination seed 1', { exact: true })).toHaveCount(0);
      await expect.poll(() => listSessionMessagesRequests.length).toBeGreaterThanOrEqual(1);
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
      await annotatePaginationProof(
        page,
        `After second scroll-to-top\nVisible: seed 1 oldest page\nListSessionMessages calls: ${listSessionMessagesRequests.length}\nGetSession calls: ${getSessionRequests.length}`,
      );
    } finally {
      await context.close();
    }
  });
});
