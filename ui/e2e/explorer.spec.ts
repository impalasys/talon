import { test, expect } from '@playwright/test';
import { createE2ETalonClient, e2eGatewayUrl, installBrowserAuth } from './talonAuth';

test.describe('Explorer navigation', () => {
  test('deep session URL auto-expands namespace path and agent', async ({ page }) => {
    const gatewayUrl = e2eGatewayUrl();
    const client = createE2ETalonClient(gatewayUrl);

    await expect(async () => {
      await client.namespaces.create({ name: 'conic:wks:13', recursive: true });
    }).toPass({ timeout: 60000 });

    await client.resources.create({
      ns: 'conic:wks:13',
      manifest: {
        apiVersion: "talon.impalasys.com/v1",
        kind: "Agent",
        metadata: { name: "cmo", namespace: "conic:wks:13", labels: {}, annotations: {}, ownerReferences: [], finalizers: [], generation: BigInt(0), resourceVersion: "", uid: "" },
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
              systemPrompt: "Explorer test",
              mcpServerRefs: [],
            },
          },
        },
      },
    });

    const sessionRes = await client.sessions.create({
      ns: 'conic:wks:13',
      agent: 'cmo',
    });

    const params = new URLSearchParams({
      connected: 'true',
      ns: 'conic:wks:13',
      type: 'session',
      agent: 'cmo',
      session: sessionRes.sessionId,
    });

    await installBrowserAuth(page, gatewayUrl);

    await page.goto(`/?${params.toString()}`);

    await expect(page.locator('text=Connected')).toBeVisible({ timeout: 15000 });
    await expect(page.locator('.truncate', { hasText: 'conic' }).first()).toBeVisible({ timeout: 15000 });
    await expect(page.locator('.truncate', { hasText: 'wks' }).first()).toBeVisible({ timeout: 15000 });
    await expect(page.locator('.truncate', { hasText: '13' }).first()).toBeVisible({ timeout: 15000 });
    await expect(page.locator('.truncate', { hasText: 'cmo' }).first()).toBeVisible({ timeout: 15000 });
  });

  test('opens schedule resource details without crashing', async ({ page }) => {
    const gatewayUrl = e2eGatewayUrl();
    const client = createE2ETalonClient(gatewayUrl);
    const testNs = `sightline-schedule-${Date.now()}`;
    const agentName = 'scheduler-agent';
    const scheduleName = 'hourly-check';
    const pageErrors: string[] = [];
    const runAt = new Date(Date.now() + 60 * 60 * 1000).toISOString().replace(/\.\d{3}Z$/, 'Z');

    page.on('pageerror', (error) => {
      pageErrors.push(error.message);
    });

    await expect(async () => {
      await client.namespaces.create({ name: testNs, recursive: true });
    }).toPass({ timeout: 60000 });

    await client.resources.create({
      ns: testNs,
      manifest: {
        apiVersion: "talon.impalasys.com/v1",
        kind: "Agent",
        metadata: { name: agentName, namespace: testNs, labels: {}, annotations: {}, ownerReferences: [], finalizers: [], generation: BigInt(0), resourceVersion: "", uid: "" },
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
              systemPrompt: "Schedule explorer test",
              mcpServerRefs: [],
            },
          },
        },
      },
    });

    await client.resources.create({
      ns: testNs,
      manifest: {
        apiVersion: "talon.impalasys.com/v1",
        kind: "Schedule",
        metadata: { name: scheduleName, namespace: testNs, labels: {}, annotations: {}, ownerReferences: [], finalizers: [], generation: BigInt(0), resourceVersion: "", uid: "" },
        spec: {
          kind: {
            case: "schedule",
            value: {
              kind: "at",
              runAt,
              timezone: "UTC",
              target: { agent: agentName, sessionMode: "new", sessionId: "", workflow: "" },
              inputMessage: "Run the scheduled explorer smoke test.",
              enabled: true,
            },
          },
        },
      },
    });

    const params = new URLSearchParams({
      connected: 'true',
      ns: testNs,
      type: 'schedule',
      name: scheduleName,
    });

    await installBrowserAuth(page, gatewayUrl);
    await page.goto(`/?${params.toString()}`);

    await expect(page.locator('text=Connected')).toBeVisible({ timeout: 15000 });
    await expect(page.getByText(scheduleName).first()).toBeVisible({ timeout: 15000 });
    await expect(page.getByRole('button', { name: 'Overview' })).toBeVisible({ timeout: 15000 });
    await expect(page.getByText('Next run')).toBeVisible({ timeout: 15000 });
    await expect(page.getByRole('button', { name: 'Raw YAML' })).toBeVisible();
    expect(pageErrors).toEqual([]);
  });

  test('opens connector config details without crashing', async ({ page }) => {
    const gatewayUrl = e2eGatewayUrl();
    const client = createE2ETalonClient(gatewayUrl);
    const testNs = `sightline-connector-${Date.now()}`;
    const agentName = 'connector-agent';
    const className = 'mock-chat';
    const connectorName = 'room-one';
    const pageErrors: string[] = [];

    page.on('pageerror', (error) => {
      pageErrors.push(error.message);
    });

    await expect(async () => {
      await client.namespaces.create({ name: testNs, recursive: true });
    }).toPass({ timeout: 60000 });

    await client.resources.create({
      ns: testNs,
      manifest: {
        apiVersion: "talon.impalasys.com/v1",
        kind: "Agent",
        metadata: { name: agentName, namespace: testNs, labels: {}, annotations: {}, ownerReferences: [], finalizers: [], generation: BigInt(0), resourceVersion: "", uid: "" },
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
              systemPrompt: "Connector explorer test",
              mcpServerRefs: [],
            },
          },
        },
      },
    });

    await client.resources.create({
      ns: testNs,
      manifest: {
        apiVersion: "talon.impalasys.com/v1",
        kind: "ConnectorClass",
        metadata: { name: className, namespace: testNs, labels: {}, annotations: {}, ownerReferences: [], finalizers: [], generation: BigInt(0), resourceVersion: "", uid: "" },
        spec: {
          kind: {
            case: "connectorClass",
            value: {
              platform: "mock",
              runtime: { kind: "http", endpoint: "http://127.0.0.1:9" },
              auth: { kind: "apiKey", apiKey: { plain: "mock-secret" } },
              matchIndexes: [{ name: "room", fields: ["roomId"] }],
            },
          },
        },
      },
    });

    await client.resources.create({
      ns: testNs,
      manifest: {
        apiVersion: "talon.impalasys.com/v1",
        kind: "Connector",
        metadata: { name: connectorName, namespace: testNs, labels: {}, annotations: {}, ownerReferences: [], finalizers: [], generation: BigInt(0), resourceVersion: "", uid: "" },
        spec: {
          kind: {
            case: "connector",
            value: {
              classRef: { name: className },
              enabled: true,
              matchFields: { roomId: "room-1" },
              consumer: {
                session: {
                  agent: { name: agentName },
                  continuity: "reuse",
                  replyMode: "hold_for_review",
                },
              },
            },
          },
        },
      },
    });

    const params = new URLSearchParams({
      connected: 'true',
      ns: testNs,
      type: 'connector',
      name: connectorName,
    });

    await installBrowserAuth(page, gatewayUrl);
    await page.goto(`/?${params.toString()}`);

    await expect(page.locator('text=Connected')).toBeVisible({ timeout: 15000 });
    await expect(page.getByText('Connectors')).toBeVisible({ timeout: 15000 });
    await expect(page.getByText(connectorName).first()).toBeVisible({ timeout: 15000 });
    const connectorYaml = page.locator('.sightline-yaml-editor .cm-content');
    await expect(connectorYaml).toBeVisible({ timeout: 15000 });
    await expect(connectorYaml).toContainText('kind: Connector', { timeout: 15000 });
    await expect(connectorYaml).toContainText('classRef', { timeout: 15000 });
    await expect(connectorYaml).toContainText('matchFields', { timeout: 15000 });
    await expect(connectorYaml).toContainText('room-1', { timeout: 15000 });
    expect(pageErrors).toEqual([]);
  });
});
