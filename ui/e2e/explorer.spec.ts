import { test, expect } from '@playwright/test';
import { createTalonClient } from "@impalasys/talon-client";

test.describe('Explorer navigation', () => {
  test('deep session URL auto-expands namespace path and agent', async ({ page }) => {
    const API_PORT = process.env.API_PORT || '50051';
    const gatewayUrl = `http://127.0.0.1:${API_PORT}`;
    const client = createTalonClient(gatewayUrl);

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

    await page.addInitScript((url) => {
      localStorage.setItem('talon_gateway_url', url);
    }, gatewayUrl);

    await page.goto(`/?${params.toString()}`);

    await expect(page.locator('text=Connected')).toBeVisible({ timeout: 15000 });
    await expect(page.locator('.truncate', { hasText: 'conic' }).first()).toBeVisible({ timeout: 15000 });
    await expect(page.locator('.truncate', { hasText: 'wks' }).first()).toBeVisible({ timeout: 15000 });
    await expect(page.locator('.truncate', { hasText: '13' }).first()).toBeVisible({ timeout: 15000 });
    await expect(page.locator('.truncate', { hasText: 'cmo' }).first()).toBeVisible({ timeout: 15000 });
  });
});
