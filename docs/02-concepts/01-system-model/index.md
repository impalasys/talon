---
title: System Model
sidebar:
  order: 1
  hidden: true
---

Talon keeps desired configuration, observed runtime state, and execution history in one durable system.

## Relationship map

Clients declare resources through the gateway. Controllers and workers reconcile or execute them; status records what Talon observed. Namespaces scope the resources, while Deployments render reusable Templates into selected namespaces.

## Read in order

1. [Control Plane Architecture](/talon/docs/concepts/system-model/control-plane-architecture)
2. [Resources and Reconciliation](/talon/docs/concepts/system-model/resources-and-reconciliation)
3. [Namespaces and Resolution](/talon/docs/concepts/system-model/namespaces-and-resolution)
4. [Templates and Deployments](/talon/docs/concepts/system-model/templates-and-deployments)

For a specific question, start with Resources and Reconciliation for state behavior or Namespaces and Resolution for scope behavior.
