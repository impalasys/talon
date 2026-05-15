---
title: Build a Marketing Agency
sidebar_position: 4
---

This tutorial shows how to model a client-services workflow in Talon using multiple roles, shared context, and recurring review loops.

## What you are building

You will build a namespace that acts like a small agency workspace for one client account.

The system will include:

- shared brand and campaign knowledge
- multiple agents for research, strategy, writing, and review
- MCP-backed tool access for external systems
- a recurring schedule for campaign review

## Talon concepts used

- namespace
- multiple agents or templates
- namespace knowledge
- MCP servers and MCP bindings
- schedules
- sessions and streamed steps
- Sightline

## Runtime surfaces used

- manifest application through `talon-cli`
- gateway CRUD for resources
- UI session surface for chat-style work
- scheduler and worker for recurring tasks

## Architecture

```text
Marketing operator
  -> Sightline or custom client
    -> Talon gateway
      -> Namespace resources for one client account
      -> Knowledge for briefs, voice, and campaign context
      -> MCP bindings for research, docs, CRM, outbound systems
      -> Scheduler for weekly review sessions
      -> Worker for execution
```

Treat each client workspace as a namespace. That keeps knowledge, schedules, and tool bindings isolated without hardcoding tenancy into the app.

## Prerequisites

- the local Talon stack is running
- you have read [Resource Model](../concepts/resource-model.md)
- you understand [Namespaces, Knowledge, and MCP](../concepts/namespaces-knowledge-and-mcp.md)

## Apply the example assets

This tutorial ships with:

- `talon/manifests/examples/marketing-agency/agency.yaml`
- `talon/manifests/examples/marketing-agency/knowledge/brand-brief.md`
- `talon/manifests/examples/marketing-agency/knowledge/campaign-plan.md`

Apply the bundle:

```bash
cd talon
cargo run --bin talon-cli -- --rest apply -f manifests/examples/marketing-agency/agency.yaml
```

The bundle models:

- one namespace per client workspace
- a strategist template
- writer and reviewer agents
- one example schedule for recurring campaign review
- placeholder MCP bindings for research and publishing tools

## Walk an end-to-end flow

Run the workflow in three steps:

1. ask the strategist agent for a weekly campaign plan
2. ask the writer agent to turn that plan into an email or landing-page draft
3. inspect the reviewer session for policy or quality feedback

Use prompts like:

- “Plan a launch-week content calendar for our analytics product”
- “Draft a lifecycle email for users who signed up but never imported data”
- “Review this campaign for tone and CTA clarity”

## Inspect and debug in Sightline

In Sightline, look at:

- the client namespace
- the knowledge resources backing the campaign context
- the MCP bindings attached to the namespace
- the review schedule and its target
- session step streams when agents call tools

If the system feels too loose:

- move client context into namespace knowledge
- narrow tool access with binding allowlists
- split roles by template so each agent has a clearer contract

## Why this structure works

This tutorial teaches an important Talon pattern:

- **namespace** for the client boundary
- **knowledge** for durable shared context
- **agents/templates** for role specialization
- **MCP bindings** for external work
- **schedules** for recurring operations

That is much cleaner than one giant “marketing agent” with every tool and every prompt baked into it.

## Extend the system

Good next steps:

- add a dedicated research agent with stronger tool access
- create separate namespaces for each customer account
- turn campaign reviews into scheduled background runs
- replace placeholder tools with real MCP-backed services

## Production notes

For a production agency workflow:

- keep tool bindings environment-specific
- store only reusable client context in knowledge
- keep approval or publishing actions outside the broadest agent role

## What you learned

You used Talon as a reusable client-workspace control plane rather than a single chat bot.

## Read next

- [Build a Customer Retention System](./customer-retention-system.md)
- [Scheduling and Background Work](../operations/scheduling-and-background-work.md)
- [Namespaces, Knowledge, and MCP](../concepts/namespaces-knowledge-and-mcp.md)
