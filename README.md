<p align="center">
  <a href="#logo-attribution">
    <img src="./docs-site/public/talon-title.svg" alt="Talon" width="260">
  </a>
</p>

<p align="center"><strong>The control plane for cloud-native agents</strong></p>

<p align="center">
  <a href="https://github.com/impalasys/talon/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/impalasys/talon?style=flat&amp;logo=github&amp;label=stars"></a>
  <a href="https://github.com/impalasys/talon/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/impalasys/talon?style=flat&amp;label=release"></a>
  <a href="#license"><img alt="License" src="https://img.shields.io/badge/license-AGPL--3.0-blue?style=flat"></a>
  <a href="https://talon.impalasys.com/docs/"><img alt="Docs" src="https://img.shields.io/badge/docs-view-0969DA?style=flat"></a>
  <a href="https://talon.impalasys.com"><img alt="Website" src="https://img.shields.io/badge/website-visit-0969DA?style=flat"></a>
</p>

Talon is a cloud-native control plane for autonomous agent fleets. It provides the infrastructure needed to operate long-lived agents in production, including durable execution, declarative configuration, namespace isolation, and a browser-native fleet view.

Agent threads survive crashes, deployments, and cold starts while prompts, tools, workflows, and policies stay explicit in a Kubernetes-style resource model.

## Capabilities

Talon bridges raw LLM inference and production-grade agent operations through a few core pillars:

- **Durable execution**: persisted sessions can resume across worker restarts, failures, deploys, and cold starts.
- **Declarative configuration**: agents, tools, workflows, knowledge, and policies are defined as YAML manifests and managed with `talon-cli`.
- **Fleet observability**: Sightline provides a browser-native view for inspecting running sessions, resource state, schedules, and execution history.
- **Extensible tooling**: namespace-scoped MCP support lets agents use approved external tools, data sources, and services.

<table>
  <tr>
    <td align="center">
      <img src="https://raw.githubusercontent.com/devicons/devicon/master/icons/amazonwebservices/amazonwebservices-original-wordmark.svg" alt="AWS" width="48" height="48"><br>
      <strong>AWS</strong>
    </td>
    <td align="center">
      <img src="https://raw.githubusercontent.com/devicons/devicon/master/icons/googlecloud/googlecloud-original.svg" alt="Google Cloud" width="48" height="48"><br>
      <strong>Google Cloud</strong>
    </td>
  </tr>
</table>

Talon uses cloud agnostic primitives and can be hosted on your own cloud. Learn more about Talon in the [docs](./docs/00-introduction.md).

## Prerequisites

To run the full Talon stack locally, make sure your environment has:

- **Docker and Docker Compose**: orchestrates the gateway, worker, Postgres, Pub/Sub emulator, Sightline UI, and bootstrap jobs.
- **Rust toolchain**: builds and runs `talon-cli`, `talon-server`, and `talon-worker` from source.
- **Protobuf compiler**: compiles the service, config, manifest, event, and model definitions used by the Rust binaries and SDKs.
- **Provider API key**: at least one real LLM provider key in `.env`, such as `OPENAI_API_KEY`.
- **Git**: clones the repository and supports normal local development workflows.

To run Talon locally, follow the [quickstart](./docs/01-getting-started/01-quickstart.md).

## Resources

Talon is organized around durable resources that can be declared, inspected, and reconciled through the gateway and `talon-cli`.

- **Namespace**: the tenancy boundary for agents, sessions, schedules, files, knowledge, and tools.
- **Template**: a reusable base for prompts, model policy, features, MCP references, and capabilities.
- **Agent**: the runtime-facing agent definition that sessions execute against.
- **Session**: durable interaction state, messages, execution steps, and lifecycle metadata.
- **File**: a namespace-visible object for memory, artifacts, search, and retrieval.
- **Knowledge**: curated context, playbooks, policies, and notes available to agents.
- **Skill**: a packaged capability or instruction surface that agents can load at runtime.
- **McpServer**: a namespace-scoped tool endpoint with transport, auth, and tool policy.
- **Channel, ChannelSubscription**: collaboration surfaces and routing rules for agent participation.
- **Schedule**: a one-shot or recurring trigger for agent/session work.
- **Workflow**: a multi-step background process with retry and output policy.
- **Task**: delegated work with assignee, lifecycle phase, progress, and result artifacts.
- **ConnectorClass, Connector**: external integration definitions and concrete event routes.
- **Deployment**: replicates resources into child namespaces.
- **SandboxClass, SandboxPolicy, Sandbox**: spin up isolated environments for agents to run code in.
- **Worker**: a system-level resource for identifying Talon worker instances and routing gRPC requests.
- **UsagePolicy**: limits and selectors for controlling runtime usage.

See the [resource model](./docs/02-concepts/03-resource-model.md) and [resource schemas](./docs/05-reference/generated/resource-schemas.md) for details.

## License

Source files in this repository are marked `AGPL-3.0-only`. See [`SECURITY.md`](./SECURITY.md) and the repository licensing metadata for additional project policy.

## Attribution

1. Logo uses the [claw icon](https://thenounproject.com/icon/claw-1849553/) by [regina](https://thenounproject.com/creator/12asbyrs/) from [Noun Project](https://thenounproject.com/).
