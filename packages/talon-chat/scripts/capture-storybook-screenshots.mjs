import { createServer } from "node:http";
import { mkdir, readFile, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const packageDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const storybookDir = path.join(packageDir, "storybook-static");
const outputDir = path.join(packageDir, "storybook-screenshots");

const stories = [
  {
    id: "talon-chat-talonsession--existing-session",
    name: "talon-session-existing",
    waitForText: "Worked for 11s",
  },
  {
    id: "talon-chat-talonsession--streaming-response",
    name: "talon-session-streaming-working",
    waitForText: "Working for",
  },
  {
    id: "talon-chat-talonsession--disabled",
    name: "talon-session-disabled",
    waitForText: "Launch update",
  },
  {
    id: "talon-chat-talonchannel--open-channel",
    name: "talon-channel-open",
    waitForText: "Rollback guardrail",
  },
  {
    id: "talon-chat-talonchannel--read-only",
    name: "talon-channel-read-only",
    waitForText: "Rollback guardrail",
  },
];

const themes = ["light", "dark"];

function contentTypeFor(filePath) {
  if (filePath.endsWith(".html")) return "text/html; charset=utf-8";
  if (filePath.endsWith(".js")) return "text/javascript; charset=utf-8";
  if (filePath.endsWith(".css")) return "text/css; charset=utf-8";
  if (filePath.endsWith(".svg")) return "image/svg+xml";
  if (filePath.endsWith(".png")) return "image/png";
  if (filePath.endsWith(".json")) return "application/json; charset=utf-8";
  if (filePath.endsWith(".woff2")) return "font/woff2";
  return "application/octet-stream";
}

async function createStaticServer(rootDir) {
  const server = createServer(async (request, response) => {
    try {
      const requestUrl = new URL(request.url || "/", "http://127.0.0.1");
      const decodedPath = decodeURIComponent(requestUrl.pathname);
      const normalizedPath = path.normalize(decodedPath).replace(/^(\.\.[/\\])+/, "");
      const filePath = path.join(rootDir, normalizedPath === "/" ? "index.html" : normalizedPath);
      const resolvedPath = path.resolve(filePath);

      if (!resolvedPath.startsWith(rootDir)) {
        response.writeHead(403);
        response.end("Forbidden");
        return;
      }

      const fileStat = await stat(resolvedPath);
      const finalPath = fileStat.isDirectory() ? path.join(resolvedPath, "index.html") : resolvedPath;
      const body = await readFile(finalPath);
      response.writeHead(200, { "Content-Type": contentTypeFor(finalPath) });
      response.end(body);
    } catch {
      response.writeHead(404);
      response.end("Not found");
    }
  });

  await new Promise((resolve) => {
    server.listen(0, "127.0.0.1", resolve);
  });

  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("Failed to start Storybook static server.");
  }

  return {
    origin: `http://127.0.0.1:${address.port}`,
    close: () => new Promise((resolve, reject) => server.close((error) => (error ? reject(error) : resolve()))),
  };
}

async function main() {
  await stat(storybookDir);
  await mkdir(outputDir, { recursive: true });

  const server = await createStaticServer(storybookDir);
  const browser = await chromium.launch();

  try {
    const page = await browser.newPage({ viewport: { width: 900, height: 900 }, deviceScaleFactor: 1 });

    for (const theme of themes) {
      for (const story of stories) {
        const url = `${server.origin}/iframe.html?id=${story.id}&viewMode=story&globals=theme:${theme}`;
        await page.goto(url, { waitUntil: "domcontentloaded" });
        await page.getByText(story.waitForText).first().waitFor({ state: "visible", timeout: 10_000 });
        await page.screenshot({
          path: path.join(outputDir, `${story.name}-${theme}.png`),
          fullPage: false,
        });
      }
    }
  } finally {
    await browser.close();
    await server.close();
  }

  console.log(`Captured ${stories.length * themes.length} Storybook screenshots in ${outputDir}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
