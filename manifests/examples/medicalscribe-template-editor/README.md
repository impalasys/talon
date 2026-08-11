# Medical Scribe template editor

This example provisions one reusable Talon Agent Template and deploys it to a
Medical Scribe namespace selected by the `environment` manifest variable.

The Deployment renders an `Agent/template-editor` in every root namespace with
the `app.medicalscribe/talon-agent: template-editor` label. `Sys` is Talon's
internal parent for root namespace references.

Apply the manifests in this order:

```bash
talon-cli apply -f manifests/examples/medicalscribe-template-editor/namespace.yaml
talon-cli apply -f manifests/examples/medicalscribe-template-editor/template.yaml
talon-cli apply -f manifests/examples/medicalscribe-template-editor/deployment.yaml
```

The namespace defaults to `scribes-prd`. Override it for another environment:

```bash
talon-cli apply \
  --var environment=staging \
  -f manifests/examples/medicalscribe-template-editor/namespace.yaml
```

Talon manifest variables use `{{ vars.environment or "prd" }}`. A literal
GitHub Actions expression such as `${{ vars.environment }}` is not evaluated
by `talon-cli`; use that form only in a workflow that supplies `--var`.

The resulting agents use Talon sessions and streaming for the conversation.
Medical Scribe remains responsible for user authorization, template
ownership, preview rendering, and saving an approved template.

The proposal block is a temporary text-level contract for the current
interactive session API. It can be replaced by Talon's structured-output
session support without changing the Agent or Deployment shape.
