import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";

import { build } from "esbuild";

const root = new URL("../../../..", import.meta.url);
const outdir = await mkdtemp(join(tmpdir(), "talon-cf-bindings-"));
const outfile = join(outdir, "bindings.test.mjs");

function runNodeTest(file) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ["--test", file], {
      cwd: root,
      stdio: "inherit",
    });
    child.on("error", reject);
    child.on("exit", (code, signal) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(signal ? `node --test exited with signal ${signal}` : `node --test exited with ${code}`));
      }
    });
  });
}

try {
  await build({
    entryPoints: [new URL("../test/bindings.test.ts", import.meta.url).pathname],
    outfile,
    bundle: true,
    format: "esm",
    platform: "node",
    target: "node22",
    sourcemap: "inline",
    logLevel: "silent",
  });
  await runNodeTest(outfile);
} finally {
  await rm(outdir, { recursive: true, force: true });
}
