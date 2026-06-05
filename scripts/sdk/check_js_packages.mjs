import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(fileURLToPath(new URL("../..", import.meta.url)));

function readJson(path) {
  return JSON.parse(readFileSync(join(root, path), "utf8"));
}

function fail(message) {
  console.error(message);
  process.exitCode = 1;
}

const server = readJson("sdk/js/talon-server/package.json");
const platformPackages = [
  {
    name: "@impalasys/talon-node-darwin-arm64",
    path: "sdk/js/talon-node-darwin-arm64/package.json",
  },
  {
    name: "@impalasys/talon-node-linux-x64",
    path: "sdk/js/talon-node-linux-x64/package.json",
  },
];

for (const platformPackage of platformPackages) {
  const pkg = readJson(platformPackage.path);
  const expected = `workspace:${pkg.version}`;
  const actual = server.optionalDependencies?.[platformPackage.name];
  if (actual !== expected) {
    fail(`${server.name} optional dependency ${platformPackage.name} is ${actual}, expected ${expected}`);
  }

  if (!pkg.files?.includes("bin") || !pkg.files?.includes("index.js")) {
    fail(`${platformPackage.name} must publish both bin and index.js`);
  }

  if (pkg.scripts?.postinstall) {
    fail(`${platformPackage.name} must not require a postinstall script`);
  }
}
