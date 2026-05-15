import {themes as prismThemes} from "prism-react-renderer";

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: "Talon Docs",
  tagline: "Builder documentation for the Talon agent control plane",
  favicon: "img/logo.svg",
  url: "https://talon.impalasys.com",
  baseUrl: "/docs/",
  organizationName: "impalasys",
  projectName: "talon",
  trailingSlash: false,
  onBrokenLinks: "throw",
  onBrokenMarkdownLinks: "warn",
  future: {
    experimental_faster: {
      rspackBundler: false,
    },
  },
  i18n: {
    defaultLocale: "en",
    locales: ["en"],
  },
  presets: [
    [
      "classic",
      {
        docs: {
          path: "../docs",
          routeBasePath: "/",
          sidebarPath: "./sidebars.mjs",
          exclude: ["wiki/**"],
          editUrl:
            "https://github.com/impalasys/talon/tree/main/docs",
        },
        blog: false,
        theme: {
          customCss: "./src/css/custom.css",
        },
      },
    ],
  ],
  themeConfig: {
    image: "img/logo.svg",
    announcementBar: {
      id: "talon-docs-shell",
      content:
        "Talon documentation is generated from the monorepo and published alongside the product site.",
      backgroundColor: "#181c24",
      textColor: "#f3f6fb",
      isCloseable: false,
    },
    navbar: {
      title: "Talon",
      logo: {
        alt: "Talon logo",
        src: "img/logo.svg",
      },
      items: [
        {to: "/", label: "Docs", position: "left"},
        {to: "/getting-started/quickstart", label: "Getting Started", position: "left"},
        {to: "/concepts/agents-and-templates", label: "Concepts", position: "left"},
        {to: "/tutorials/first-agent", label: "Tutorials", position: "left"},
        {to: "/reference", label: "Reference", position: "left"},
        {to: "/operations/local-development", label: "Operations", position: "left"},
        {to: "/contributing/docs-system", label: "Contributing", position: "left"},
        {href: "https://talon.impalasys.com", label: "Website", position: "right"},
        {href: "https://github.com/impalasys/talon", label: "GitHub", position: "right"},
      ],
    },
    footer: {
      style: "dark",
      links: [
        {
          title: "Start",
          items: [
            {label: "Quickstart", to: "/getting-started/quickstart"},
            {label: "Architecture", to: "/getting-started/architecture"},
          ],
        },
        {
          title: "Reference",
          items: [
            {label: "Gateway API", to: "/reference/generated/gateway-service"},
            {label: "Manifest schema", to: "/reference/generated/manifests-schema"},
            {label: "Config schema", to: "/reference/generated/config-schema"},
          ],
        },
        {
          title: "Project",
          items: [
            {label: "Website", href: "https://talon.impalasys.com"},
            {label: "GitHub", href: "https://github.com/impalasys/talon"},
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Impala Systems, Inc.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
    },
  },
};

export default config;
