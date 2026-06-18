# ACP Agents

This example creates a self-contained `Example` namespace with a Docker-backed
ACP coding agent.

Build the local sandbox image before starting a session:

```bash
docker build -f dockerfiles/codex-acp.Dockerfile -t talon-codex-acp:local .
```

Then apply the manifests:

```bash
talon-cli apply -f manifests/examples/acp-agents
```

The `coding` agent runs `codex-acp` inside a sandbox leased from the `coding`
`SandboxPolicy`. Provide `OPENAI_API_KEY` to the Talon worker or Docker runtime
environment; do not commit API keys into these manifests.
