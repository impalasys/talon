# ACP Agents

This example creates a self-contained `Example` namespace with a Docker-backed
ACP coding agent.

Build the local sandbox image before starting a session:

```bash
docker build -f dockerfiles/codex-acp.Dockerfile -t talon-acp-harness:local .
```

Then apply the manifests:

```bash
talon-cli apply -f manifests/examples/acp-agents
```

The `coding` agent uses `harnessRef: codex`, which resolves to `codex-acp`
inside a sandbox leased from the `coding` `SandboxPolicy`. The same image also
contains Claude Code and OpenCode ACP harnesses:

- `claude-code` resolves to `claude-code-acp`
- `opencode` resolves to `opencode acp`

Provide the matching credential to the Talon worker or Docker runtime
environment, for example `OPENAI_API_KEY`, `CODEX_API_KEY`, `ANTHROPIC_API_KEY`,
or `OPENCODE_API_KEY`. Do not commit API keys into these manifests.
