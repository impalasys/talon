---
title: Build a Customer Retention System
sidebar:
  order: 5
---

This tutorial makes Talon’s scheduler concrete by pairing a real agent with a real schedule managed as a Talon resource.

Before you begin, clone the repository, create `.env` from `.env.example`, and set `OPENAI_API_KEY` so the example agent uses a real model provider:

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

You will create:

- a `customer-retention` namespace
- a `retention-reviewer` agent
- shared retention knowledge
- a recurring schedule that dispatches review work into the worker

## 1. Apply the agent resources

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/customer-retention-system/namespace.yaml
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/customer-retention-system/retention-review-template.yaml
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f manifests/examples/customer-retention-system/retention-reviewer.yaml
```

## 2. Sync the retention knowledge

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 knowledge sync \
  --namespace customer-retention \
  --dir manifests/examples/customer-retention-system/knowledge
```

This loads:

- `retention-playbook.md`
- `health-model.md`

## 3. Create a schedule

Write a schedule manifest:

```bash
cat > /tmp/weekly-retention-review.schedule.yaml <<'EOF'
apiVersion: talon.impalasys.com/v1
kind: Schedule
metadata:
  name: weekly-retention-review
  namespace: customer-retention
  labels:
    tutorial: customer-retention
spec:
  kind: cron
  cron: "0 * * * *"
  timezone: UTC
  target:
    agent: retention-reviewer
    sessionMode: new
  inputMessage: Review customer health signals and propose next actions.
  enabled: true
EOF
```

Apply it through the typed gateway:

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 apply -f /tmp/weekly-retention-review.schedule.yaml
```

That example runs hourly so you can observe it locally without waiting long.

## 4. Verify the schedule exists

```bash
cargo run --bin talon-cli -- --gateway http://localhost:50051 get schedule weekly-retention-review --namespace customer-retention
```

Look for:

- `spec.enabled`
- `status.backendArmed`
- `status.nextRunAt`

## 5. Inspect the results in Sightline

In Sightline:

- open the `customer-retention` namespace
- inspect the `weekly-retention-review` schedule
- watch for sessions created by the scheduled run

This is the important Talon behavior: scheduled work lands in the same durable session model as interactive work.

## Troubleshooting

- If the schedule exists but never runs, verify the local stack is still running and the worker is healthy.
- If the schedule targets the wrong agent, edit the manifest and re-run `talon-cli apply`.
- If you need an immediate test, also create a manual session against `retention-reviewer` and compare the resulting transcript to the scheduled runs.

## Why this tutorial matters

This is where Talon stops looking like “chat with memory” and starts looking like a real automation control plane.

## Read next

- [Build an Internal Ops Copilot](./internal-ops-copilot)
- [Scheduling and Background Work](../operations/scheduling-and-background-work)
