import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, mkdir, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tempRoot = await mkdtemp(join(tmpdir(), "talon-chat-package-"));

try {
  const packDir = join(tempRoot, "pack");
  const extractDir = join(tempRoot, "extract");
  await mkdir(packDir);
  await mkdir(extractDir);

  execFileSync("pnpm", ["pack", "--pack-destination", packDir], {
    cwd: packageRoot,
    stdio: "inherit",
  });

  const archives = (await readdir(packDir)).filter((name) => name.endsWith(".tgz"));
  assert.equal(archives.length, 1, `expected one package archive, found ${archives.length}`);
  execFileSync("tar", ["-xzf", join(packDir, archives[0]), "-C", extractDir], { stdio: "inherit" });

  const packedRoot = join(extractDir, "package");
  const manifest = JSON.parse(await readFile(join(packedRoot, "package.json"), "utf8"));
  const entrypoints = new Set([manifest.main, manifest.module, manifest.types]);
  collectExportTargets(manifest.exports, entrypoints);

  for (const entrypoint of entrypoints) {
    if (typeof entrypoint !== "string") continue;
    const absolutePath = resolve(packedRoot, entrypoint);
    assert.ok(
      absolutePath.startsWith(`${packedRoot}/`),
      `package entrypoint escapes the archive: ${entrypoint}`,
    );
    await readFile(absolutePath);
  }

  const runtimeFiles = await findRuntimeFiles(join(packedRoot, "dist"));
  assert.ok(runtimeFiles.length > 0, "package archive contains no JavaScript runtime files");
  const invalidImports = [];
  const typedImportPattern = /(?:\bfrom\s*|\bimport\s*(?:\(\s*)?|\brequire\s*\()\s*["']([^"']+\.(?:ts|tsx|mts|cts)(?:[?#][^"']*)?)["']/g;
  for (const file of runtimeFiles) {
    const source = await readFile(file, "utf8");
    for (const match of source.matchAll(typedImportPattern)) {
      invalidImports.push(`${relative(packedRoot, file)} imports ${match[1]}`);
    }
  }
  assert.deepEqual(invalidImports, [], `package contains TypeScript runtime imports:\n${invalidImports.join("\n")}`);

  await symlink(join(packageRoot, "node_modules"), join(packedRoot, "node_modules"), "dir");
  const esmSmoke = join(packedRoot, "package-smoke.mjs");
  const cjsSmoke = join(packedRoot, "package-smoke.cjs");
  await writeFile(
    esmSmoke,
    `import * as pkg from ${JSON.stringify(manifest.name)};\n` +
      `if (typeof pkg.TalonSession !== "function") throw new Error("ESM TalonSession export is missing");\n`,
  );
  await writeFile(
    cjsSmoke,
    `const pkg = require(${JSON.stringify(manifest.name)});\n` +
      `if (typeof pkg.TalonSession !== "function") throw new Error("CommonJS TalonSession export is missing");\n`,
  );
  execFileSync(process.execPath, [esmSmoke], { stdio: "inherit" });
  execFileSync(process.execPath, [cjsSmoke], { stdio: "inherit" });

  console.log(`Verified packed ${manifest.name}@${manifest.version} (${runtimeFiles.length} runtime files).`);
} finally {
  await rm(tempRoot, { recursive: true, force: true });
}

function collectExportTargets(value, targets) {
  if (typeof value === "string") {
    targets.add(value);
    return;
  }
  if (!value || typeof value !== "object") return;
  for (const child of Object.values(value)) collectExportTargets(child, targets);
}

async function findRuntimeFiles(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...await findRuntimeFiles(path));
    } else if (/\.(?:js|cjs|mjs)$/.test(entry.name)) {
      files.push(path);
    }
  }
  return files;
}
