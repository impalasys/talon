---
title: Events and Models
sidebar_position: 3
---

This page summarizes the most important runtime events and stored models.

## Session events

The worker and gateway cooperate through event streams that represent:

- session message dispatch
- session control actions
- lifecycle changes to runtime resources

## Session step events

`SessionStepEvent` is the core live execution signal. A step includes:

- session identity
- step type
- content
- timestamp
- optional step name
- structured payload JSON

The common step types are:

- token
- action
- observation
- done
- error

## Stored models

The most important persisted runtime models are:

- `Agent`
- `Session`
- `Schedule`
- `Namespace`
- `Knowledge`

## Why this page exists

Generated schema pages are useful for exact fields, but these event and model types are important enough to call out explicitly in the public docs narrative.

## Read next

- [Runtime Surfaces](./runtime-surfaces.md)
- [Gateway API](./generated/gateway-service.md)
