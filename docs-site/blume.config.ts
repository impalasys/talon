import { defineConfig } from "blume";

export default defineConfig({
  title: "Talon Docs",
  description: "Builder documentation for the Talon agent control plane.",
  logo: {
    image: "/docs-logo.svg",
    text: "Talon",
  },
  basePath: "/talon/docs",
  redirects: [{ from: "/", to: "/build", status: 308 }],
  content: {
    root: "../docs",
    exclude: ["**/_*", "**/.*", "wiki/**", "99-drafts/**"],
  },
  github: {
    owner: "impalasys",
    repo: "talon",
    branch: "main",
    dir: "talon/docs-site",
  },
  navigation: {
    tabs: [
      { label: "Build", path: "/build", icon: "rocket" },
      { label: "Concepts", path: "/concepts", icon: "book-open" },
      { label: "Operate", path: "/operate", icon: "settings" },
      { label: "Reference", path: "/reference", icon: "braces" },
    ],
    featured: [
      { label: "Product site", href: "https://talon.impalasys.com", icon: "globe" },
    ],
    sidebar: {
      display: "group",
    },
  },
  deployment: {
    output: "static",
    site: "https://talon.impalasys.com",
  },
  markdown: {
    imageZoom: true,
    code: {
      icons: true,
      wrap: false,
    },
    codeBlocks: {
      theme: {
        light: "github-light",
        dark: "github-dark",
      },
    },
  },
  ai: {
    llmsTxt: true,
  },
  seo: {
    og: { enabled: true },
    sitemap: true,
    robots: true,
    structuredData: true,
  },
});
