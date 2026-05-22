This directory contains example assets referenced by the public Talon tutorials.

The files are organized by tutorial:

- `chatgpt-app/`
- `marketing-agency/`
- `customer-retention-system/`
- `internal-ops-copilot/`

Each example directory can include:

- one or more manifest files for namespaces, templates, agents, and bindings
- sample knowledge documents
- notes about which runtime surfaces need additional setup

These assets are intended to be instructional starting points for the local Talon stack.

Current conventions:

- each YAML manifest file is a separate `talon-cli apply` input
- markdown under `knowledge/` is loaded with `talon-cli knowledge sync`
- schedules are created through the gateway API, because `talon-cli apply` does not currently support `Schedule` manifests
