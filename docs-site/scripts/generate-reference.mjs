import {mkdir, readFile, rm, writeFile} from "node:fs/promises";
import path from "node:path";
import {fileURLToPath} from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const talonRoot = path.resolve(__dirname, "..", "..");
const docsRoot = path.resolve(talonRoot, "docs");
const generatedRoot = path.resolve(docsRoot, "reference", "generated");
const protoRoot = path.resolve(talonRoot, "proto");

const apiProtos = [
  "auth.proto",
  "channels.proto",
  "connectors.proto",
  "knowledge.proto",
  "namespaces.proto",
  "resources.proto",
  "search.proto",
  "sessions.proto",
  "workflows.proto",
].map((file) => path.resolve(protoRoot, "talon", "v1", file));
const configProto = path.resolve(protoRoot, "config.proto");
const externalProtos = [
  "external/connectors.proto",
].map((file) => path.resolve(protoRoot, file));
const resourceProtos = [
  "resources/common.proto",
  "resources/agents.proto",
  "resources/mcp.proto",
  "resources/knowledge.proto",
  "resources/namespaces.proto",
  "resources/channels.proto",
  "resources/connectors.proto",
  "resources/schedules.proto",
  "resources/workflows.proto",
  "resources/deployments.proto",
  "resources/sandboxes.proto",
  "resources/sessions.proto",
  "resources/skills.proto",
  "resources/usage.proto",
  "resources/workers.proto",
  "resources/resource.proto",
  "data/routing.proto",
].map((file) => path.resolve(protoRoot, file));

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

- \`talon/proto/talon/v1/*.proto\`
- \`talon/proto/config.proto\`
- \`talon/proto/resources/*.proto\`
- \`talon/proto/data/*.proto\`
- \`talon/proto/external/*.proto\`

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
  sourcePaths: externalProtos,
  title: "External Connector Schemas",
  slug: "external-connector-schemas",
  intro:
    "This page summarizes the connector runtime contract that external connector services implement when registering clusters, receiving deliveries and activities, and calling back into Talon.",
});
await generateSchemaReference({
  sourcePaths: resourceProtos,
  title: "Resource Schemas",
  slug: "resource-schemas",
  intro:
    "This page summarizes the control-plane resource messages that drive Talon agents, deployments, sandbox orchestration, MCP servers, schedules, workflows, and knowledge resources.",
});

async function generateGatewayReference() {
  const proto = (await Promise.all(apiProtos.map((file) => readFile(file, "utf8")))).join("\n");
  const serviceNames = [
    "NamespaceService",
    "ResourceService",
    "SessionService",
    "ChannelService",
    "WorkflowService",
    "KnowledgeService",
    "AuthService",
    "ConnectorService",
    "SearchService",
  ];
  const services = serviceNames.map((name) => ({
    name,
    methods: parseServiceMethods(extractBlock(proto, "service", name)),
  }));
  const methodCount = services.reduce((count, service) => count + service.methods.length, 0);

  const lines = [
    "---",
    "title: Talon v1 Services",
    "sidebar_position: 2",
    "---",
    "",
    "The Talon gateway API is defined by the domain service files in `proto/talon/v1/*.proto`. They are the canonical first-class gRPC and gRPC-Web contract exposed directly by the gateway.",
    "",
    "## Surface summary",
    "",
    `- Package: \`talon.v1\``,
    `- Services: ${serviceNames.map((name) => `\`${name}\``).join(", ")}`,
    `- Transport modes: native gRPC and gRPC-Web on the gateway port`,
    `- Total RPC methods: **${methodCount}**`,
    "",
  ];

  for (const service of services) {
    if (!service.methods.length) {
      continue;
    }

    lines.push(`## ${service.name}`, "");
    for (const method of service.methods) {
      lines.push(`### \`${method.name}\``, "");
      if (method.comment) {
        lines.push(method.comment, "");
      }
      lines.push(`- Request: \`${method.request}\``);
      lines.push(`- Response: \`${method.response}\`${method.stream ? " (server stream)" : ""}`);
      lines.push("");
    }
  }

  await writeFile(path.join(generatedRoot, "gateway-service.md"), lines.join("\n"));
}

async function generateSchemaReference({sourcePath, sourcePaths, title, slug, intro}) {
  const paths = sourcePaths ?? [sourcePath];
  const protoParts = await Promise.all(paths.map((file) => readFile(file, "utf8")));
  const proto = protoParts.join("\n");
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

    const rpcMatch = line.match(/^rpc\s+(\w+)\(([\w.]+)\)\s+returns\s+\((stream\s+)?([\w.]+)\)\s*([;{])\s*$/);
    if (!rpcMatch) {
      continue;
    }

    const bodyLines = [];
    if (rpcMatch[5] === "{") {
      let depth = 1;
      while (depth > 0 && index + 1 < lines.length) {
        index += 1;
        const bodyLine = lines[index];
        depth += count(bodyLine, "{");
        depth -= count(bodyLine, "}");
        bodyLines.push(bodyLine);
      }
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
  if (path.includes("/mcp-servers")) return "MCP";
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
