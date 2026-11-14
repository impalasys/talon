import { createRequire } from "node:module";
import { dirname } from "node:path";
const require = createRequire(import.meta.url);
const packageRoot = (name: string) => dirname(require.resolve(`${name}/package.json`));

import type { StorybookConfig } from "@storybook/react-vite";

const config: StorybookConfig = {
  stories: ["../src/**/*.stories.@(ts|tsx)"],
  addons: [packageRoot("@storybook/addon-docs")],
  framework: {
    name: packageRoot("@storybook/react-vite"),
    options: {},
  },
  docs: {
    autodocs: "tag",
  },
};

export default config;
