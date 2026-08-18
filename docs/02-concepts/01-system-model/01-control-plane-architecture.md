---
title: Control Plane Architecture
sidebar:
  order: 1
---

## In brief

Talon is an agent control plane: it accepts intent, retains it as durable state, makes that state operable through one API, and records what happened when work runs. It is not a stateless chat endpoint or an agent SDK alone. A conversation, schedule, workflow, or deployment is useful precisely because Talon can inspect it after the client that created it has gone away.

The control-plane model separates a declaration of work from the processes that perform it. A caller can create an Agent, submit a turn, or declare a Deployment through the same boundary. Talon stores the declaration first; workers and controllers then act on it. That separation makes retries, recovery, inspection, and access control properties of the system rather than obligations every client must rebuild.

## What it owns

The gateway is the public boundary for resources, sessions, streams, and administration. Its job is to authenticate and authorize callers, validate requests, persist accepted intent, and expose both durable reads and live event surfaces. It is the control plane's interface—not a worker proxy that asks a process to keep a request open.

Workers perform agent turns and background work. Controllers converge the resource families they own. The control plane supplies the durable stores, work and event delivery, scheduler, and object storage that let those processes operate without becoming the source of truth themselves. [Sightline](../../build/using-sightline) is an operator interface over that same gateway-backed state; it does not hold a parallel model of the runtime.

Talon deliberately does not own a caller's entire application. A product still chooses its UX, domain database, and external integrations. Talon owns the agent-system state and execution contracts that those applications need to create, observe, and govern.

## How work moves

At a high level, a client submits intent to the gateway; the gateway writes durable state and makes work available; a worker or controller claims the work and performs it; then Talon records the outcome and publishes live updates. A stream is useful while that work is running, but it is not the authority for its result. Later readers use the committed state.

This yields two related but distinct loops:

- **Reconciliation** brings declared resource state toward observed state—for example, rendering a deployment into selected namespaces.
- **Execution** claims a discrete piece of runtime work—for example, running an agent turn or a workflow step.

The distinction matters when investigating a system. A resource can be accepted and still be reconciling. A session can have a durable submitted turn even when no client is currently streaming it. Neither situation means a caller needs to repeat the original request.

## Boundaries and tradeoffs

Centralizing the boundary gives Talon one place to apply identity, access, usage limits, and versioned service contracts. The tradeoff is intentional: clients work through the control plane rather than treating a particular worker as their API server. That makes a worker replaceable and lets operators inspect a single coherent history, but it also means product-specific workflow and UI decisions remain outside Talon.

This page describes stable responsibilities, not a request-by-request trace. [Execution and Recovery](../execution-model/execution-and-recovery) follows the worker sequence. [Resources and Reconciliation](./resources-and-reconciliation) explains how declared and observed state relate.

## Read next

- [Resources and Reconciliation](./resources-and-reconciliation)
- [Gateway and Clients](../boundary-and-policy-model/gateway-and-clients)
