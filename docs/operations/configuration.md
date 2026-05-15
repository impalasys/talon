---
title: Configuration
sidebar_position: 4
---

Talon configuration covers providers, the control plane, and scheduler behavior.

## Provider configuration

Provider config defines model backends and secrets. The config schema supports:

- OpenAI
- Anthropic
- Google
- generic OpenAI-compatible providers

## Secret sources

Secrets can be sourced from:

- plain inline values
- environment variables
- GCP Secret Manager
- local keychain
- AWS or Azure secret references

## Control plane configuration

The control plane config defines:

- database driver and URL
- message broker driver
- scheduler backend configuration

## Local environment

The local compose stack sets most runtime wiring automatically, including:

- Postgres URL
- Pub/Sub emulator host
- local scheduler driver
- worker pull mode

Provider credentials usually come from `.env`, environment variables, or local keychain lookup in `run.sh`.

## Read next

- [Local Development](./local-development.md)
- [Config Schema](../reference/generated/config-schema.md)
