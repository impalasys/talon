---
title: Templates and Deployments
sidebar:
  order: 4
---

## In brief

Templates are reusable declarative resource definitions. Deployments select namespaces and reconcile rendered Template output through DeploymentReplicas. Together they are Talon's declarative distribution mechanism: define a resource once, choose where it should exist, then observe each concrete placement.

This is deliberately a resource-level mechanism. A Template can describe a target resource kind, its metadata, and a templated specification. It is not a base class for Agents, and changing a Template is not agent inheritance or a delta applied at runtime.

## What it owns

A Template owns reusable desired content: a target resource kind, metadata, and its templated `spec`. A Deployment owns placement intent: which Template names should be rendered into the namespaces it selects. A `DeploymentReplica` represents one concrete rendered placement and records the result Talon observed.

The rendered resource remains an ordinary resource. Its controller and status follow the rules of its own kind; the Deployment does not make it a hidden special case. This separation lets a user inspect both the distribution relationship and the resulting resource instead of conflating them.

## Reconciliation and updates

When a Deployment, its selected namespace set, or an included Template changes, Talon reconciles the desired replica set. It renders the Template for each selected target and records `DeploymentReplica` state. Removed placement or changed input similarly changes the desired set that reconciliation must bring into line.

That flow is intentionally observable rather than magical: the Template captures reusable intent, the Deployment captures where intent should appear, and the replica makes a particular target placement inspectable. The normal resource lifecycle then tells whether the generated resource has reached its own ready state.

## Relationships and tradeoffs

Templates and Deployments are valuable when the same resource family should be applied consistently across namespace boundaries. They trade some local autonomy for centrally declared distribution; namespaces can still have their own resources and policies, and a selector makes placement explicit.

Do not use this model to describe one agent's runtime behavior. [Agents and Runtimes](../execution-model/agents-and-runtimes) owns an Agent's prompts, model policy, capabilities, and runtime selection. Use a Deployment when the question is “where should this declarative resource be rendered?”—not “how does this agent inherit configuration?”

## Read next

- [Agents and Runtimes](../execution-model/agents-and-runtimes)
- [Resources and Reconciliation](./resources-and-reconciliation)
