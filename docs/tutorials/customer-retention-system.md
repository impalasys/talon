---
title: Build a Customer Retention System
sidebar_position: 5
---

This tutorial uses Talon for recurring health checks, follow-up recommendations, and operator review.

## What you are building

You will build a retention workflow that:

- stores retention playbooks and policies as knowledge
- runs a retention agent on a recurring schedule
- uses MCP-backed tools to inspect customer or support context
- creates durable sessions for every review run
- lets operators inspect the resulting work in Sightline

## Talon concepts used

- namespace
- agent
- knowledge
- MCP bindings
- schedules
- worker execution
- session history
- Sightline

## Runtime surfaces used

- manifest application through `talon-cli`
- schedule CRUD through the gateway
- worker execution triggered by the scheduler
- Sightline for run inspection

## Architecture

```text
Scheduler
  -> Gateway schedule callback
    -> Worker claims due work
      -> Retention agent session
        -> Optional MCP calls to CRM, support, messaging systems
        -> Persisted messages and step history
          -> Operator review in Sightline
```

This is the tutorial that makes the scheduler and worker feel concrete. The important idea is that Talon stores recurring work in the same control plane as sessions and agents.

## Prerequisites

- the local Talon stack is running
- you have read [Scheduling and Background Work](../operations/scheduling-and-background-work.md)
- you know where to inspect sessions in Sightline

## Apply the example assets

This tutorial ships with:

- `talon/manifests/examples/customer-retention-system/retention.yaml`
- `talon/manifests/examples/customer-retention-system/knowledge/retention-playbook.md`
- `talon/manifests/examples/customer-retention-system/knowledge/health-model.md`

Apply the bundle:

```bash
cd talon
cargo run --bin talon-cli -- --rest apply -f manifests/examples/customer-retention-system/retention.yaml
```

The bundle defines:

- a retention namespace
- one health-review agent
- a recurring schedule
- placeholder MCP bindings for CRM and support lookups

## Walk an end-to-end flow

Use this narrative:

1. the schedule fires a recurring customer-health review
2. the worker creates or resumes the review session
3. the agent inspects available context and suggests follow-up actions
4. an operator reviews the result in Sightline

Good prompts for manual runs:

- “Review this account for churn risk and recommend next steps”
- “Summarize the retention risk indicators from the last 30 days”
- “Draft an outreach plan for a customer with falling activity”

## Inspect and debug in Sightline

Inspect:

- the schedule resource and its status
- the resulting review sessions
- tool activity during the run
- whether the schedule is enabled and armed

If runs do not happen:

- verify the local scheduler backend is running with the stack
- verify the schedule exists in the expected namespace
- verify the target agent still resolves

## Why this structure works

Retention systems are usually part chat, part workflow automation. Talon supports both:

- interactive sessions for operators
- recurring scheduled work for background reviews

That makes it a good fit for operational AI systems, not just chat interfaces.

## Extend the system

Good next steps:

- add separate schedules for onboarding, risk review, and renewal prep
- split recommendations by segment or plan type
- add approval loops before outbound communication
- connect real CRM or messaging tools through MCP

## Production notes

In production, be explicit about:

- scheduler auth and callback configuration
- idempotency for recurring actions
- tool scoping for anything that can send or modify external data

## What you learned

You used Talon as a durable automation system with scheduled background execution, not only an interactive assistant.

## Read next

- [Build an Internal Ops Copilot](./internal-ops-copilot.md)
- [Authentication and Access](../operations/authentication-and-access.md)
- [Scheduling and Background Work](../operations/scheduling-and-background-work.md)
