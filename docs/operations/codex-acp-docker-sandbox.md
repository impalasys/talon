# Codex ACP Docker Sandbox

Talon does not depend on a third-party Codex ACP Docker image. Build the local sandbox image from this repository, then reference that image from a `SandboxClass`.

```bash
docker build \
  -f dockerfiles/codex-acp.Dockerfile \
  -t talon-codex-acp:local \
  .
```

Main branch publishes the same image to GHCR:

```text
ghcr.io/impalasys/talon-codex-acp:latest
ghcr.io/impalasys/talon-codex-acp:sha-<git-sha>
```

The image contains:

- OpenAI Codex CLI from `@openai/codex`
- Zed's Codex ACP adapter from `@zed-industries/codex-acp`
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
    image: talon-codex-acp:local
  credentials: {}
```

When Talon leases a Docker sandbox from a `SandboxPolicy` that points at this class, the Docker backend starts the container and the ACP runtime launches `codex-acp` inside it.

The opt-in smoke test uses the same default image:

```bash
TALON_CODEX_ACP_TEST=1 cargo test harness::acp::tests::codex_acp_starts_inside_docker_sandbox_when_enabled
```

Override the image with `TALON_CODEX_ACP_IMAGE` when testing a registry-published build. Set `TALON_CODEX_ACP_PLATFORM` only when you intentionally want Docker to pull or run a specific image platform.
