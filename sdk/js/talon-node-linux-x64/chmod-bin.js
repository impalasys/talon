import { chmodSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const binary = join(__dirname, "bin", "talon-node");

try {
  chmodSync(binary, 0o755);
} catch (error) {
  if (error?.code !== "ENOENT") throw error;
}
