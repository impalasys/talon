import type React from "react";
import type { Preview } from "@storybook/react-vite";

const preview: Preview = {
  globalTypes: {
    theme: {
      description: "Preview theme",
      defaultValue: "light",
      toolbar: {
        icon: "mirror",
        items: [
          { value: "light", title: "Light" },
          { value: "dark", title: "Dark" },
        ],
        dynamicTitle: true,
      },
    },
  },
  decorators: [
    (Story, context) => {
      const isDark = context.globals.theme === "dark";
      const themeStyle = {
        height: "100vh",
        overflow: "hidden",
        background: isDark ? "#09090b" : "#fafafa",
        color: isDark ? "#fafafa" : "#18181b",
        colorScheme: isDark ? "dark" : "light",
        "--background": isDark ? "#09090b" : "#ffffff",
        "--foreground": isDark ? "#fafafa" : "#18181b",
        "--talon-chat-surface": isDark ? "#18181b" : "#ffffff",
        "--talon-chat-border": isDark ? "rgba(212,212,216,0.24)" : "rgba(212,212,216,0.7)",
        "--talon-chat-muted-fg": isDark ? "rgba(212,212,216,0.68)" : "rgba(82,82,91,0.88)",
        "--talon-chat-subtle-fg": isDark ? "rgba(161,161,170,0.78)" : "rgba(113,113,122,0.9)",
        "--talon-chat-divider": isDark ? "rgba(212,212,216,0.14)" : "rgba(212,212,216,0.7)",
        "--talon-chat-code-bg": isDark ? "rgba(255,255,255,0.06)" : "rgba(24,24,27,0.05)",
        "--talon-chat-user-bubble-bg": isDark ? "rgba(255,255,255,0.09)" : "rgba(24,24,27,0.07)",
        "--talon-chat-composer-bg": isDark
          ? "linear-gradient(to top, rgba(24,24,27,0.96), rgba(24,24,27,0.78) 58%, rgba(24,24,27,0))"
          : "linear-gradient(to top, rgba(255,255,255,0.94), rgba(255,255,255,0.72) 58%, rgba(255,255,255,0))",
        "--talon-chat-scrollbar-thumb": isDark ? "rgba(212,212,216,0.38)" : "rgba(113,113,122,0.52)",
        "--talon-chat-avatar-bg": isDark ? "#fafafa" : "#18181b",
        "--talon-chat-avatar-fg": isDark ? "#18181b" : "#ffffff",
        "--copilot-input-bg": isDark ? "rgba(39,39,42,0.94)" : "rgba(255,255,255,0.96)",
        "--copilot-input-border": isDark ? "rgba(212,212,216,0.22)" : "rgba(212,212,216,0.72)",
        "--copilot-input-placeholder": isDark ? "rgba(212,212,216,0.56)" : "rgba(82,82,91,0.64)",
        "--copilot-input-shadow": isDark ? "0 4px 18px rgba(0,0,0,0.34), 0 1px 2px rgba(0,0,0,0.28)" : "0 2px 8px rgba(24,24,27,0.06), 0 1px 2px rgba(24,24,27,0.08)",
        "--copilot-channel-input-bg": isDark ? "rgba(39,39,42,0.88)" : "rgba(255,255,255,0.72)",
        "--copilot-channel-message-bg": isDark ? "rgba(39,39,42,0.82)" : "rgba(255,255,255,0.72)",
      } as React.CSSProperties;

      return (
        <div style={themeStyle}>
          <Story />
        </div>
      );
    },
  ],
  parameters: {
    backgrounds: {
      default: "light",
      values: [
        { name: "light", value: "#fafafa" },
        { name: "dark", value: "#09090b" },
      ],
    },
    controls: {
      matchers: {
        color: /(background|color)$/i,
        date: /Date$/i,
      },
    },
    layout: "fullscreen",
  },
};

export default preview;
