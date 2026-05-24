import { defineConfig } from "tsup";

export default defineConfig({
  entry: ["src/index.ts"],
  format: ["esm", "cjs"],
  outDir: "dist",
  sourcemap: true,
  clean: true,
  splitting: false,
  bundle: true,
  minify: false,
  target: "es2020",
  external: ["react", "react-dom", "lucide-react", "streamdown"],
});
