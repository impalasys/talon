---
title: Channels and Subscriptions
sidebar:
  order: 2
---

## In brief

A Channel is a durable shared conversation. A ChannelSubscription defines when a public channel message should create work for an agent.

## Ownership

The Channel owns public messages and context policy. A subscription identifies an agent and declares routing behavior such as its trigger, recent public context, and reply mode. The agent still performs work in a separate, private Session.

## Lifecycle

Posting a matching channel message causes Talon to route work to the subscribed agent's session. Normal assistant output stays private. An agent publishes a public response only through the channel-publish capability, which keeps public collaboration distinct from private execution history.

## Relationships

A Channel is not an Agent Session: it is the shared conversation that can initiate one or more sessions. Connectors can deliver external input to a Channel, and Tasks or Workflows are separate coordination mechanisms.

## Read next

- [Sessions and Submissions](../execution-model/sessions-and-submissions)
- [Connectors and Conversations](../boundary-and-policy-model/connectors-and-conversations)
