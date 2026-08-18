---
title: Boundary and Policy Model
sidebar:
  order: 5
  hidden: true
---

Talon controls the boundaries where clients, external systems, identities, and resource limits meet the control plane.

## Relationship map

Clients use the gateway to read and change Talon state. Connectors translate external conversations at a controlled ingress and egress. Identity decides who may act, while UsagePolicy constrains the resources an allowed action may consume.

## Read in order

1. [Gateway and Clients](/talon/docs/concepts/boundary-and-policy-model/gateway-and-clients)
2. [Connectors and Conversations](/talon/docs/concepts/boundary-and-policy-model/connectors-and-conversations)
3. [Identity, Access, and Secrets](/talon/docs/concepts/boundary-and-policy-model/identity-access-and-secrets)
4. [Usage and Limits](/talon/docs/concepts/boundary-and-policy-model/usage-and-limits)

Start with Gateway and Clients for an integration boundary or Identity, Access, and Secrets for trust boundaries.
