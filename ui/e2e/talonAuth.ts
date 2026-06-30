import fs from 'node:fs';
import path from 'node:path';
import type { Page } from '@playwright/test';
import { createTalonClient } from '@impalasys/talon-client';

type E2EAuth = {
  gatewayUrl?: string;
  apiKey?: string;
  accessToken?: string;
  expiresAt?: number;
};

export function readE2EAuth(): E2EAuth | null {
  const authFile = process.env.TALON_E2E_AUTH_FILE
    || path.resolve(process.cwd(), '..', 'target', 'talon-e2e-auth.json');
  if (!fs.existsSync(authFile)) {
    return null;
  }
  return JSON.parse(fs.readFileSync(authFile, 'utf8')) as E2EAuth;
}

export function e2eGatewayUrl() {
  const auth = readE2EAuth();
  if (auth?.gatewayUrl) {
    return auth.gatewayUrl;
  }
  const apiPort = process.env.API_PORT || '50051';
  return `http://127.0.0.1:${apiPort}`;
}

export function createE2ETalonClient(gatewayUrl = e2eGatewayUrl()) {
  const auth = readE2EAuth();
  if (auth?.apiKey) {
    return createTalonClient({ baseUrl: gatewayUrl, apiKey: auth.apiKey });
  }
  return createTalonClient(gatewayUrl);
}

export async function installBrowserAuth(page: Page, gatewayUrl = e2eGatewayUrl()) {
  const auth = readE2EAuth();
  await page.addInitScript(
    ({ url, token }) => {
      localStorage.setItem('talon_gateway_url', url);
      if (token) {
        localStorage.setItem('talon_auth_token', token);
      }
    },
    { url: gatewayUrl, token: auth?.accessToken || null },
  );
}
