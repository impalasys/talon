---
title: Resources and Reconciliation
sidebar:
  order: 2
---

## In brief

Talon represents durable configuration as versioned resources. A resource carries `apiVersion`, `kind`, metadata, desired `spec`, and observed `status`. That common envelope lets one system manage long-lived declarations such as Agents, Templates, and Policies alongside the state that says whether they have taken effect.

The important boundary is between a caller's intention and Talon's observation. A client says what it wants in `spec`; controllers and runtime services report what actually happened in `status`. A successful write is therefore not a promise that every downstream effect has already completed.

## What it owns

Metadata gives the resource its identity, namespace, labels, and ownership relationships. The desired specification is the stable input to the component responsible for the resource family. Status is controller-owned output: it can include conditions, runtime facts, and the generation of the specification that was last observed.

`generation` changes when desired state changes. `observedGeneration` lets a controller say which desired generation its status describes. Conditions provide a readable statement of readiness or failure without asking clients to infer it from implementation details. Exact fields vary by kind and belong in the generated [Resource Schemas](../../reference/generated/resource-schemas).

Not every resource has the same lifecycle. Some are declarative inputs to reconciliation, some record a durable runtime entity, and some carry policy or routing data. The shared envelope creates a common way to inspect them; it does not imply every resource has a dedicated continuous controller.

## Reconciliation

Creating, updating, or deleting a resource changes durable state and can notify the controllers responsible for that kind. A controller reads the relevant desired state, performs only the work it owns, and writes an observation back. For example:

- a Deployment controller renders Templates into selected namespaces and records `DeploymentReplica` state;
- connector resources publish the routing information needed for external conversation dispatch;
- sandbox policy and runtime state guide the realization and leasing of isolated execution.

This is convergence, not a synchronous call stack from an API write to every outcome. A controller may need to retry, and status tells an operator whether it has caught up. Runtime execution uses the same durable-state discipline but follows a claim-and-commit lifecycle instead; see [Execution and Recovery](../execution-model/execution-and-recovery).

## Relationships and tradeoffs

Resources make desired state portable, inspectable, and safe to reconcile repeatedly. In return, consumers must distinguish an accepted declaration from its observed effect. Use a resource when the thing should be named, retained, governed, and visible over time. Use a session submission or workflow run when the concern is one execution rather than an enduring declaration.

Namespaces provide the primary scope for resources. [Templates and Deployments](./templates-and-deployments) shows how a declaration can fan out without turning Templates into a special Agent inheritance mechanism.

## Read next

- [Namespaces and Resolution](./namespaces-and-resolution)
- [Templates and Deployments](./templates-and-deployments)
