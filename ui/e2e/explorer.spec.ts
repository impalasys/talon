import { test, expect } from '@playwright/test';
import { createClient } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { GatewayService } from "../proto/proto/gateway_pb";

test.describe('Explorer navigation', () => {
  test('deep session URL auto-expands namespace path and agent', async ({ page }) => {
    const API_PORT = process.env.API_PORT || '18789';
    const gatewayUrl = `http://127.0.0.1:${API_PORT}`;
    const client = createClient(GatewayService, createGrpcWebTransport({ baseUrl: gatewayUrl }));

    await expect(async () => {
      await client.createNamespace({ name: 'conic:wks:13', recursive: true });
    }).toPass({ timeout: 60000 });

    await client.createAgent({
      ns: 'conic:wks:13',
      name: 'cmo',
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
            systemPrompt: "Explorer test",
            mcpServerRefs: [],
          },
        },
      },
    });

    const sessionRes = await client.createSession({
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

    await page.goto(`/?${params.toString()}`);

    await expect(page.locator('text=Connected')).toBeVisible({ timeout: 15000 });
    await expect(page.locator('.truncate', { hasText: 'conic' }).first()).toBeVisible({ timeout: 15000 });
    await expect(page.locator('.truncate', { hasText: 'wks' }).first()).toBeVisible({ timeout: 15000 });
    await expect(page.locator('.truncate', { hasText: '13' }).first()).toBeVisible({ timeout: 15000 });
    await expect(page.locator('.truncate', { hasText: 'cmo' }).first()).toBeVisible({ timeout: 15000 });
  });
});
