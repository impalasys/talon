---
title: Build a Marketing Agency
sidebar_position: 4
---

This tutorial shows how to model a multi-role workspace around shared knowledge and distinct agents.

Before you begin, clone the repository, create `.env` from `.env.example`, and set `OPENAI_API_KEY` so the example agents use a real model provider:

```bash
git clone https://github.com/impalasys/talon.git
cd talon
cp .env.example .env
```

Start the local stack if it is not already running:

```bash
docker compose up --build -d
```

## What you are building

You will create a `marketing-agency` namespace with:

- shared brand and campaign knowledge
- a `campaign-writer` agent
- a `campaign-reviewer` agent
- separate session histories for drafting and review

This tutorial stays inside real surfaces that exist in the repo. It does not pretend there are working publishing tools or schedules in the example assets.

## 1. Apply the workspace resources

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest apply -f manifests/examples/marketing-agency/namespace.yaml
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest apply -f manifests/examples/marketing-agency/strategist-template.yaml
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest apply -f manifests/examples/marketing-agency/campaign-writer.yaml
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest apply -f manifests/examples/marketing-agency/campaign-reviewer.yaml
```

That creates:

- the `marketing-agency` namespace
- the `strategist-template` template
- the `campaign-writer` agent
- the `campaign-reviewer` agent

## 2. Sync the shared knowledge

```bash
cargo run --bin talon-cli -- --gateway http://localhost:18789 --rest knowledge sync \
  --namespace marketing-agency \
  --dir manifests/examples/marketing-agency/knowledge
```

The example files are:

- `brand-brief.md`
- `campaign-plan.md`

## 3. Start a drafting session

Create a session for the writer:

```bash
curl -sS http://localhost:18789/v1/ns/marketing-agency/agents/campaign-writer/sessions \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"ns":"marketing-agency","agent":"campaign-writer"}'
```

Send a prompt through the UI session API:

```bash
curl -sS http://localhost:18789/v1/ui/ns/marketing-agency/agents/campaign-writer/sessions/<writer-session-id> \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"messages":[{"content":"Draft a launch email for the campaign in our plan."}]}'
```

## 4. Start a review session

Create a separate session for the reviewer:

```bash
curl -sS http://localhost:18789/v1/ns/marketing-agency/agents/campaign-reviewer/sessions \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"ns":"marketing-agency","agent":"campaign-reviewer"}'
```

Then ask for critique:

```bash
curl -sS http://localhost:18789/v1/ui/ns/marketing-agency/agents/campaign-reviewer/sessions/<review-session-id> \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"messages":[{"content":"Review the launch email for tone, positioning, and CTA clarity."}]}'
```

## 5. Inspect the workspace in Sightline

Look at:

- the shared knowledge in `marketing-agency`
- the separate writer and reviewer agents
- the distinct session histories for drafting and critique

This is the Talon pattern that matters here: shared durable context at the namespace level, but separate execution history per role.

## Why this structure works

- namespace knowledge holds the reusable client context
- templates keep shared role behavior in one place
- separate agents give you cleaner transcripts and clearer responsibility boundaries

That is easier to operate than one giant all-purpose marketing agent.

## Read next

- [Build a Customer Retention System](./customer-retention-system.md)
- [Namespaces, Knowledge, and MCP](../concepts/namespaces-knowledge-and-mcp.md)
