import { rmSync } from "node:fs";
import { defineConfig, type Options } from "tsup";

rmSync("dist", { recursive: true, force: true });

const common: Options = {
  entry: ["src/index.ts"],
  outDir: "dist",
  sourcemap: true,
  splitting: false,
  bundle: true,
  minify: false,
  target: "es2020",
};

const external = ["@impalasys/talon-client", "react", "react-dom", "lucide-react"];

export default defineConfig([
  {
    ...common,
    format: ["esm"],
    clean: false,
    external: [...external, "streamdown"],
  },
  {
    ...common,
    format: ["cjs"],
    clean: false,
    external,
    noExternal: ["streamdown"],
  },
]);
