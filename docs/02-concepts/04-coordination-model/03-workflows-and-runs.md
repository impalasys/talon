---
title: Workflows and Runs
sidebar:
  order: 3
---

## In brief

A Workflow is a namespaced declaration of dependent steps. A WorkflowRun is the durable execution of that declaration.

## Ownership

The Workflow owns the declared graph: input and output shape, dependencies and conditions, steps, retries, timeouts, and concurrency policy. A WorkflowRun owns the concrete progress, step results, and events for one execution.

## Lifecycle

Talon creates a run, evaluates runnable steps in dependency order, persists step outcomes, and exposes run events. Steps can invoke agents, tools, child workflows, transforms, or waits. A caller can resume or cancel a run through the workflow service.

## Relationships

Workflows make orchestration explicit and inspectable. They differ from a Session, where an agent decides dynamically, and from a Task, which tracks an owner/delegate responsibility rather than a graph.

## Read next

- [Tasks and Delegation](./tasks-and-delegation)
- [Gateway and Clients](../boundary-and-policy-model/gateway-and-clients)
