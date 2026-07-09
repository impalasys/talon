---
title: Scheduling and Background Work
sidebar_position: 3
---

Schedules let Talon trigger agent work later or repeatedly.

## What a schedule does

A schedule stores:

- when work should run
- which namespace it belongs to
- which agent or session it targets
- the input message and enablement state

## Execution model

The scheduler does not execute agent work directly.

Instead:

1. a scheduler backend decides when a schedule should fire
2. the worker receives a wakeup
3. the worker claims the runnable schedule
4. the worker dispatches the actual session work

## Local mode

In the default Docker compose stack, Talon uses the `local_postgres` scheduler backend with a shared secret for wakeup authentication.

For a same-host SQLite deployment, Talon also supports `local_sqlite`. In that mode, the scheduler stores wakeups in the same local SQLite database used by the control plane.

Use:

- `TALON_SCHEDULER_DRIVER=local_postgres` for the current local Postgres stack
- `TALON_SCHEDULER_DRIVER=local_sqlite` for a same-host SQLite deployment

Both local backends expect the gateway and worker to access the database from the same machine.

In the smallest same-host setup, Talon can pair `local_sqlite` scheduling with the `local_socket` message broker so wakeups and worker dispatch both stay local to the machine.

## Cloud mode

The codebase also supports Cloud Tasks-backed scheduler callbacks with either:

- shared-secret auth
- Google OIDC callback auth

On AWS, Talon supports EventBridge Scheduler-backed wakeups. EventBridge Scheduler creates one-time wakeups and targets the Talon SQS worker queue; Talon still owns recurrence, claiming, status updates, and re-arming. Use `TALON_SCHEDULER_DRIVER=aws_eventbridge_scheduler` with SQS worker pull mode.

LocalStack can validate EventBridge Scheduler create/get/delete calls, but its Scheduler implementation does not fire targets into SQS. Use Talon's local scheduler backends for deterministic end-to-end schedule execution tests, and keep real AWS Scheduler-to-SQS smoke tests opt-in.

## Why this matters

Schedules are part of the same durable control plane as sessions and agents. They are not an external cron wrapper bolted onto the side.

## Read next

- [Configuration](./configuration.md)
- [Deployment Model](./deployment-model.md)
