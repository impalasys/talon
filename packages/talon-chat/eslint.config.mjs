import sonarjs from "eslint-plugin-sonarjs";
import tseslint from "typescript-eslint";

export default [{
  files: ["src/**/*.{ts,tsx}"],
  ignores: ["dist/**"],
  languageOptions: {
    parser: tseslint.parser,
    parserOptions: { ecmaVersion: "latest", sourceType: "module", ecmaFeatures: { jsx: true } },
  },
  plugins: { sonarjs },
  rules: {
    complexity: ["warn", 12],
    "sonarjs/cognitive-complexity": ["warn", 15],
    "max-depth": ["warn", 4],
    "max-lines-per-function": ["warn", { max: 120, skipBlankLines: true, skipComments: true }],
    "max-lines": ["warn", { max: 600, skipBlankLines: true, skipComments: true }],
  },
}];
