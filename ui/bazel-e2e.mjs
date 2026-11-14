import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
const [backend, frontend, playwright, ...args] = process.argv.slice(2);
const quote = (value) => "'" + value.replaceAll("'", "'\\''") + "'";
const env = {...process.env,
  BACKEND_COMMAND: quote(resolve(backend)), FRONTEND_COMMAND: 'cd .. && ' + quote(resolve(frontend)) + ' preview --host 0.0.0.0 --port 3000 --strictPort',
  TALON_E2E_AUTH_FILE: `${process.env.TEST_TMPDIR}/auth.json`,
  SIGHTLINE_SCREENSHOT_DIR: process.env.TEST_UNDECLARED_OUTPUTS_DIR,
  REUSE_EXISTING_SERVER: 'false',
};
const result = spawnSync(resolve(playwright), ['test', '--config=ui/playwright.config.ts', ...args], {stdio:'inherit', env});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
