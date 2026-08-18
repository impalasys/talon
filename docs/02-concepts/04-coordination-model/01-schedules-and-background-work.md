---
title: Schedules and Background Work
sidebar:
  order: 1
---

## In brief

A Schedule is a durable instruction to dispatch work once or repeatedly. It is a trigger, not the executor of that work.

## Ownership

A Schedule records when work should run, its namespace, enabled state, input, and target. It can target a new or existing agent Session or a Workflow. The worker performs the actual execution after the scheduler wakes it.

## Lifecycle

The scheduler backend arms a wakeup. A worker receives it, claims runnable schedule work, dispatches the target, records status and events, and re-arms recurrence when appropriate. Schedule status makes the next run, past outcomes, and errors inspectable.

## Relationships

Use a Schedule for time; use a Workflow for a declared multi-step graph and a Task for delegated responsibility. Scheduler backend configuration belongs in [Operate](../../operate/scheduling-and-background-work).

## Read next

- [Workflows and Runs](./workflows-and-runs)
- [Tasks and Delegation](./tasks-and-delegation)
