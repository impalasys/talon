---
title: Skills
sidebar:
  order: 2
---

## In brief

A Skill is a namespaced package of reusable agent guidance. It makes a capability discoverable before an agent activates its instructions and supporting files for a session.

## Ownership

The Skill resource declares the package. Its required entrypoint is a File at `/skills/<skill>/SKILL.md`; related package files can accompany it. Skills belong to namespace context rather than to one hard-coded Agent.

## Lifecycle

Skill catalogs resolve through namespace ancestry, with a child skill shadowing a same-named parent skill. An agent can discover the catalog and activate a skill when it needs the package's context. Activation is runtime context selection, not a change to the Agent resource.

## Relationships

Skills package instructions and files; they are not MCP servers or sandbox definitions. [Tools, MCP, and Sandboxes](./tools-mcp-and-sandboxes) covers executable surfaces, and [Namespaces and Resolution](../system-model/namespaces-and-resolution) covers the shared resolution rule.

## Read next

- [Tools, MCP, and Sandboxes](./tools-mcp-and-sandboxes)
- [Namespaces and Resolution](../system-model/namespaces-and-resolution)
