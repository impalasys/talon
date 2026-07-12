import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..');

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': __dirname,
      '@impalasys/talon-chat': path.resolve(repoRoot, 'packages/talon-chat/src/index.ts'),
    },
  },
  server: {
    host: '0.0.0.0',
    port: 3000,
    allowedHosts: ['.trycloudflare.com'],
    fs: {
      allow: [repoRoot],
    },
  },
  define: {
    'process.env.NEXT_PUBLIC_GATEWAY_URL': JSON.stringify(
      process.env.VITE_GATEWAY_URL ?? process.env.NEXT_PUBLIC_GATEWAY_URL ?? '',
    ),
    'process.env.NEXT_PUBLIC_TALON_OBJECT_API_URL': JSON.stringify(
      process.env.VITE_TALON_OBJECT_API_URL ?? process.env.NEXT_PUBLIC_TALON_OBJECT_API_URL ?? '',
    ),
  },
});
