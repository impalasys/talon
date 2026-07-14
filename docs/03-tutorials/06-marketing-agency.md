---
title: Build a Marketing Agency
sidebar:
  order: 4
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
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/marketing-agency/namespace.yaml
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/marketing-agency/strategist-template.yaml
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/marketing-agency/campaign-writer.yaml
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/marketing-agency/campaign-reviewer.yaml
```

That creates:

- the `marketing-agency` namespace
- the `strategist-template` template
- the `campaign-writer` agent
- the `campaign-reviewer` agent

## 2. Sync the shared knowledge

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 knowledge sync \
  --namespace marketing-agency \
  --dir manifests/examples/marketing-agency/knowledge
```

The example files are:

- `brand-brief.md`
- `campaign-plan.md`

## 3. Start a drafting session

Create a writer session and stream the draft:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 session prompt \
  --namespace marketing-agency \
  --agent campaign-writer \
  --stream \
  "Draft a launch email for the campaign in our plan."
```

## 4. Start a review session

Create a separate reviewer session and ask for critique:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 session prompt \
  --namespace marketing-agency \
  --agent campaign-reviewer \
  --stream \
  "Review the launch email for tone, positioning, and CTA clarity."
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

- [Build a Customer Retention System](./customer-retention-system)
- [Namespaces, Knowledge, and MCP](../concepts/namespaces-knowledge-and-mcp)
