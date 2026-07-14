# ACP Harness Docker Sandbox

Talon does not depend on third-party ACP Docker images. Build the local sandbox image from this repository, then reference that image from a `SandboxClass`.

```bash
docker build \
  -f dockerfiles/codex-acp.Dockerfile \
  -t talon-acp-harness:local \
  .
```

Main branch publishes the same image to GHCR:

```text
ghcr.io/impalasys/talon-acp-harness:latest
ghcr.io/impalasys/talon-acp-harness:sha-<git-sha>
ghcr.io/impalasys/talon-codex-acp:latest
ghcr.io/impalasys/talon-codex-acp:sha-<git-sha>
ghcr.io/impalasys/talon-claude-code-acp:latest
ghcr.io/impalasys/talon-claude-code-acp:sha-<git-sha>
ghcr.io/impalasys/talon-opencode-acp:latest
ghcr.io/impalasys/talon-opencode-acp:sha-<git-sha>
```

The image contains:

- OpenAI Codex CLI from `@openai/codex`
- Zed's Codex ACP adapter from `@zed-industries/codex-acp`
- Anthropic Claude Code CLI from `@anthropic-ai/claude-code`
- Zed's Claude Code ACP adapter from `@zed-industries/claude-code-acp`
- OpenCode CLI from `opencode-ai`
- Common coding tools: Git, SSH client, Python, ripgrep, jq, curl, and a build toolchain

Do not bake API keys into the image. Pass credentials at runtime through the Talon sandbox backend or local Docker environment.

For the local company-builder example, apply:

```bash
talon-cli apply -f manifests/examples/v2-company-builder/sandbox-class-docker.yaml
```

That manifest uses:

```yaml
apiVersion: talon.impalasys.com/v1
kind: SandboxClass
metadata:
  name: docker-code
  namespace: system
spec:
  provider: docker
  providerConfig:
    image: talon-acp-harness:local
  credentials: {}
```

When Talon leases a Docker sandbox from a `SandboxPolicy` that points at this class, the Docker backend starts the container and the ACP runtime launches the configured ACP harness inside it.

Talon recognizes these `harnessRef` aliases:

```yaml
runtime:
  kind: acp
  acp:
    harnessRef: codex
    cwd: /workspace
    sandboxPolicyRef: coding
```

```yaml
runtime:
  kind: acp
  acp:
    harnessRef: claude-code
    cwd: /workspace
    sandboxPolicyRef: coding
```

```yaml
runtime:
  kind: acp
  acp:
    harnessRef: opencode
    cwd: /workspace
    sandboxPolicyRef: coding
```

`codex` resolves to `codex-acp`, `claude-code` resolves to `claude-code-acp`, and `opencode` resolves to `opencode acp`. Talon forwards matching process environment keys when present: `OPENAI_API_KEY` and `CODEX_API_KEY` for Codex, `ANTHROPIC_API_KEY` for Claude Code, and `OPENCODE_API_KEY` plus common provider keys for OpenCode.

The opt-in smoke test uses the same default image:

```bash
TALON_CODEX_ACP_TEST=1 cargo test harness::acp::tests::codex_acp_starts_inside_docker_sandbox_when_enabled
```

Override the image with `TALON_CODEX_ACP_IMAGE` when testing a registry-published build. Set `TALON_CODEX_ACP_PLATFORM` only when you intentionally want Docker to pull or run a specific image platform.

The live Python e2e can run any supported harness:

```bash
TALON_ACP_DOCKER_E2E=1 \
TALON_ACP_E2E_HARNESS=codex \
TALON_ACP_E2E_IMAGE=ghcr.io/impalasys/talon-acp-harness:latest \
pytest tests/test_chat_sqlite.py -k live_acp
```

For the pytest path, set `TALON_ACP_E2E_HARNESS` to `codex`, `claude-code`, or `opencode`, and provide the matching credential in the environment or repo `.env`: `CODEX_API_KEY`/`OPENAI_API_KEY` for Codex, `ANTHROPIC_API_KEY` for Claude Code, or `OPENCODE_API_KEY` plus provider keys for OpenCode. The legacy `TALON_CODEX_DOCKER_E2E` and `TALON_CODEX_ACP_IMAGE` variables still work for Codex mode.
