import { defineConfig } from "blume";

export default defineConfig({
  title: "Talon Docs",
  description: "Builder documentation for the Talon agent control plane.",
  logo: {
    image: "/docs-logo.svg",
    text: "Talon",
  },
  basePath: "/talon/docs",
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
      { label: "Getting Started", path: "/getting-started", icon: "rocket" },
      { label: "Concepts", path: "/concepts", icon: "book-open" },
      { label: "Tutorials", path: "/tutorials", icon: "graduation-cap" },
      { label: "Reference", path: "/reference", icon: "braces" },
      { label: "Operations", path: "/operations", icon: "settings" },
      { label: "Contributing", path: "/contributing/docs-system", icon: "git-pull-request" },
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
