import {mkdir, readFile, rm, writeFile} from "node:fs/promises";
import path from "node:path";
import {fileURLToPath} from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const talonRoot = path.resolve(__dirname, "..", "..");
const docsRoot = path.resolve(talonRoot, "docs");
const generatedRoot = path.resolve(docsRoot, "reference", "generated");
const protoRoot = path.resolve(talonRoot, "proto");

const gatewayProto = path.resolve(protoRoot, "gateway.proto");
const configProto = path.resolve(protoRoot, "config.proto");
const manifestsProto = path.resolve(protoRoot, "manifests.proto");

await rm(generatedRoot, {recursive: true, force: true});
await mkdir(generatedRoot, {recursive: true});

await writeFile(
  path.join(generatedRoot, "_category_.json"),
  JSON.stringify({label: "Generated reference", position: 99}, null, 2) + "\n",
);

await writeFile(
  path.join(generatedRoot, "index.md"),
  `---
title: Generated Reference
sidebar_position: 1
---

This section is generated from Talon's canonical source files in the monorepo:

- \`talon/proto/gateway.proto\`
- \`talon/proto/config.proto\`
- \`talon/proto/manifests.proto\`

The generated pages are intentionally static artifacts checked into the repo so API changes are reviewable in pull requests.
`,
);

await generateGatewayReference();
await generateSchemaReference({
  sourcePath: configProto,
  title: "Config Schema",
  slug: "config-schema",
  intro:
    "This page summarizes the major configuration messages exposed by Talon's runtime configuration proto.",
});
await generateSchemaReference({
  sourcePath: manifestsProto,
  title: "Manifest Schema",
  slug: "manifests-schema",
  intro:
    "This page summarizes the manifest types that drive Talon agents, templates, MCP servers, bindings, and knowledge resources.",
});

async function generateGatewayReference() {
  const proto = await readFile(gatewayProto, "utf8");
  const serviceBlock = extractBlock(proto, "service", "GatewayService");
  const methods = parseServiceMethods(serviceBlock);

  const sectionOrder = [
    "Agents",
    "Knowledge",
    "Sessions",
    "Schedules",
    "Namespaces",
    "Templates",
    "MCP",
    "Other",
  ];

  const grouped = new Map();
  for (const section of sectionOrder) {
    grouped.set(section, []);
  }
  for (const method of methods) {
    grouped.get(classifyMethod(method))?.push(method);
  }

  const lines = [
    "---",
    "title: Gateway Service",
    "sidebar_position: 2",
    "---",
    "",
    "The Talon gateway is defined in `proto/gateway.proto`. It is the canonical contract for both gRPC and the REST-transcoded HTTP surface exposed through the gateway and Envoy.",
    "",
    "## Surface summary",
    "",
    `- Service: \`talon.gateway.GatewayService\``,
    `- Transport modes: gRPC, gRPC-web, REST via \`google.api.http\` annotations, and the browser-oriented \`/v1/ui/... \` stream path documented separately in the hand-written guides`,
    `- Total RPC methods: **${methods.length}**`,
    "",
  ];

  for (const section of sectionOrder) {
    const entries = grouped.get(section);
    if (!entries?.length) {
      continue;
    }

    lines.push(`## ${section}`, "");
    for (const method of entries) {
      lines.push(`### \`${method.name}\``, "");
      if (method.comment) {
        lines.push(method.comment, "");
      }
      lines.push(`- Request: \`${method.request}\``);
      lines.push(`- Response: \`${method.response}\`${method.stream ? " (server stream)" : ""}`);
      if (method.http) {
        lines.push(`- REST mapping: \`${method.http.verb.toUpperCase()} ${method.http.path}\``);
        if (method.http.body) {
          lines.push(`- REST body: \`${method.http.body}\``);
        }
      } else {
        lines.push("- REST mapping: none");
      }
      lines.push("");
    }
  }

  await writeFile(path.join(generatedRoot, "gateway-service.md"), lines.join("\n"));
}

async function generateSchemaReference({sourcePath, title, slug, intro}) {
  const proto = await readFile(sourcePath, "utf8");
  const messages = parseTopLevelMessages(proto);

  const lines = [
    "---",
    `title: ${title}`,
    "---",
    "",
    intro,
    "",
  ];

  for (const message of messages) {
    lines.push(`## \`${message.name}\``, "");
    if (message.comment) {
      lines.push(message.comment, "");
    }

    if (!message.fields.length) {
      lines.push("_No top-level fields documented._", "");
      continue;
    }

    lines.push("| Field | Type | Notes |", "| --- | --- | --- |");
    for (const field of message.fields) {
      const notes = [field.label, field.groupComment].filter(Boolean).join("; ");
      lines.push(`| \`${field.name}\` | \`${field.type}\` | ${notes || "-"} |`);
    }
    lines.push("");
  }

  await writeFile(path.join(generatedRoot, `${slug}.md`), lines.join("\n"));
}

function parseServiceMethods(serviceBlock) {
  const lines = serviceBlock.split("\n");
  const methods = [];
  let commentBuffer = [];
  let pendingSection = null;

  for (let index = 0; index < lines.length; index += 1) {
    const raw = lines[index];
    const line = raw.trim();

    if (line.startsWith("//")) {
      const text = line.replace(/^\/\/\s?/, "").trim();
      if (text && isSectionLabel(text)) {
        pendingSection = text;
        commentBuffer = [];
      } else if (text) {
        commentBuffer.push(text);
      }
      continue;
    }

    const rpcMatch = line.match(/^rpc\s+(\w+)\(([\w.]+)\)\s+returns\s+\((stream\s+)?([\w.]+)\)\s*\{$/);
    if (!rpcMatch) {
      continue;
    }

    const bodyLines = [];
    let depth = 1;
    while (depth > 0 && index + 1 < lines.length) {
      index += 1;
      const bodyLine = lines[index];
      depth += count(bodyLine, "{");
      depth -= count(bodyLine, "}");
      bodyLines.push(bodyLine);
    }

    const body = bodyLines.join("\n");
    const httpVerbMatch = body.match(/\b(get|post|put|delete):\s*"([^"]+)"/);
    const bodyMatch = body.match(/\bbody:\s*"([^"]+)"/);

    methods.push({
      section: pendingSection,
      name: rpcMatch[1],
      request: rpcMatch[2],
      response: rpcMatch[4],
      stream: Boolean(rpcMatch[3]),
      comment: commentBuffer.join(" "),
      http: httpVerbMatch
        ? {
            verb: httpVerbMatch[1],
            path: httpVerbMatch[2],
            body: bodyMatch?.[1] || "",
          }
        : null,
    });

    commentBuffer = [];
  }

  return methods;
}

function parseTopLevelMessages(proto) {
  const lines = proto.split("\n");
  const messages = [];
  let depth = 0;
  let commentBuffer = [];
  let current = null;
  let currentOneof = null;
  let oneofComment = "";

  for (const raw of lines) {
    const line = raw.trim();

    if (!line) {
      continue;
    }

    if (line.startsWith("//")) {
      commentBuffer.push(line.replace(/^\/\/\s?/, "").trim());
      continue;
    }

    if (depth === 0) {
      const messageMatch = line.match(/^message\s+(\w+)\s*\{$/);
      if (messageMatch) {
        current = {
          name: messageMatch[1],
          comment: commentBuffer.join(" "),
          fields: [],
        };
        commentBuffer = [];
      } else {
        commentBuffer = [];
      }
    } else if (depth === 1 && current) {
      const oneofMatch = line.match(/^oneof\s+(\w+)\s*\{$/);
      if (oneofMatch) {
        currentOneof = oneofMatch[1];
        oneofComment = commentBuffer.join(" ");
        commentBuffer = [];
      } else {
        const field = parseFieldLine(line);
        if (field) {
          current.fields.push({
            ...field,
            groupComment: currentOneof ? `${oneofComment || "oneof"} (${currentOneof})` : commentBuffer.join(" "),
          });
        }
        commentBuffer = [];
      }
    } else if (depth === 2 && current && currentOneof) {
      const field = parseFieldLine(line);
      if (field) {
        current.fields.push({
          ...field,
          groupComment: `${oneofComment || "oneof"} (${currentOneof})`,
        });
      }
      commentBuffer = [];
    }

    const openCount = count(raw, "{");
    const closeCount = count(raw, "}");
    depth += openCount;
    depth -= closeCount;

    if (currentOneof && depth <= 1) {
      currentOneof = null;
      oneofComment = "";
    }

    if (current && depth === 0) {
      messages.push(current);
      current = null;
    }
  }

  return messages;
}

function parseFieldLine(line) {
  const fieldMatch = line.match(/^(repeated\s+|optional\s+)?(map<[^>]+>|[\w.]+)\s+(\w+)\s*=\s*\d+/);
  if (!fieldMatch) {
    return null;
  }
  return {
    label: fieldMatch[1]?.trim() || "",
    type: fieldMatch[2],
    name: fieldMatch[3],
  };
}

function extractBlock(proto, kind, name) {
  const lines = proto.split("\n");
  const start = lines.findIndex((line) => line.trim().startsWith(`${kind} ${name}`));
  if (start === -1) {
    throw new Error(`Unable to find ${kind} ${name}`);
  }

  const blockLines = [];
  let depth = 0;
  let started = false;

  for (let index = start; index < lines.length; index += 1) {
    const line = lines[index];
    blockLines.push(line);
    depth += count(line, "{");
    depth -= count(line, "}");
    if (line.includes("{")) {
      started = true;
    }
    if (started && depth === 0) {
      break;
    }
  }

  return blockLines.join("\n");
}

function classifyMethod(method) {
  const path = method.http?.path || "";
  if (path.includes("/schedules")) return "Schedules";
  if (path.includes("/knowledge")) return "Knowledge";
  if (path.includes("/sessions")) return "Sessions";
  if (path.includes("/templates")) return "Templates";
  if (path.includes("/mcp-servers") || path.includes("/mcp-bindings")) return "MCP";
  if (path.includes("/namespaces") && !path.includes("/agents")) return "Namespaces";
  if (path.includes("/agents")) return "Agents";
  return "Other";
}

function isSectionLabel(value) {
  return [
    "Agent Lifecycle",
    "Agent Knowledge",
    "Agent Sessions",
    "Interactive Comm",
    "Schedules",
    "Namespaces",
    "Agent Templates",
    "MCP Servers",
  ].includes(value);
}

function count(value, token) {
  const cleanValue = value.replace(/\/\/.*$/, "");
  return cleanValue.split(token).length - 1;
}
