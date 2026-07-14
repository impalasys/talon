import { expect, test } from '@playwright/test';
import { e2eGatewayUrl, readE2EAuth } from './talonAuth';

const SIGHTLINE_REFRESH_URL_COOKIE_NAME = 'sightline_refresh_url';
const RUNTIME_AUTH_TOKEN_STORAGE_KEY = 'talon_auth_token';

function grpcWebTrailers(headers: Record<string, string>) {
  const trailerBlock = `${Object.entries(headers)
    .map(([name, value]) => `${name}: ${value}`)
    .join('\r\n')}\r\n`;
  const trailerBytes = Buffer.from(trailerBlock, 'utf8');
  const frame = Buffer.alloc(5 + trailerBytes.length);
  frame[0] = 0x80;
  frame.writeUInt32BE(trailerBytes.length, 1);
  trailerBytes.copy(frame, 5);
  return frame;
}

test.describe('Sightline auth refresh', () => {
  test('refreshes from the Osprey cookie and retries when the gateway rejects an expired JWT', async ({ page }) => {
    const auth = readE2EAuth();
    expect(auth?.accessToken, 'E2E stack must provide a valid replacement access token').toBeTruthy();

    const gatewayUrl = e2eGatewayUrl();
    const webPort = process.env.WEB_PORT || '3000';
    const appOrigin = `http://localhost:${webPort}`;
    const refreshUrl = `${appOrigin}/internal/v1/sightline/refresh`;
    const expiredToken = 'expired-platform-jwt';
    let refreshCalls = 0;
    let namespaceListCalls = 0;
    let firstAuthorizationHeader: string | undefined;
    let retryAuthorizationHeader: string | undefined;

    await page.addInitScript(
      ({ gatewayUrl: initGatewayUrl, token }) => {
        localStorage.setItem('talon_gateway_url', initGatewayUrl);
        localStorage.setItem('talon_auth_token', token);
      },
      { gatewayUrl, token: expiredToken },
    );
    await page.context().addCookies([
      {
        name: SIGHTLINE_REFRESH_URL_COOKIE_NAME,
        value: encodeURIComponent(refreshUrl),
        url: appOrigin,
        sameSite: 'Lax',
      },
    ]);

    await page.route('**/internal/v1/sightline/refresh', async (route) => {
      refreshCalls += 1;
      expect(route.request().method()).toBe('POST');
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ accessToken: auth?.accessToken }),
      });
    });

    await page.route('**/talon.v1.NamespaceService/List', async (route) => {
      namespaceListCalls += 1;
      const authorization = route.request().headers().authorization;
      if (namespaceListCalls === 1) {
        firstAuthorizationHeader = authorization;
        await route.fulfill({
          status: 200,
          headers: { 'content-type': 'application/grpc-web+proto' },
          body: grpcWebTrailers({
            'grpc-status': '16',
            'grpc-message': encodeURIComponent('Invalid token: invalid platform JWT: ExpiredSignature'),
          }),
        });
        return;
      }

      retryAuthorizationHeader = authorization;
      await route.continue();
    });

    await page.goto('/?connected=true');

    await expect(page.locator('text=Connected')).toBeVisible({ timeout: 45000 });
    await expect.poll(() => refreshCalls).toBe(1);
    await expect.poll(() => namespaceListCalls).toBeGreaterThanOrEqual(2);
    expect(firstAuthorizationHeader).toBe(`Bearer ${expiredToken}`);
    expect(retryAuthorizationHeader).toBe(`Bearer ${auth?.accessToken}`);
    await expect.poll(
      () => page.evaluate((key) => localStorage.getItem(key), RUNTIME_AUTH_TOKEN_STORAGE_KEY),
    ).toBe(auth?.accessToken);
  });
});
