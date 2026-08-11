# Medical Scribe template editor

This example provisions one reusable Talon Agent Template and deploys it to
the production and staging Medical Scribe namespaces.

The Deployment renders an `Agent/template-editor` in every root namespace with
the `medicalscribe.dev/talon-agent: template-editor` label. `Sys` is Talon's
internal parent for root namespace references; the two namespace manifests
provide the production and staging targets.

Apply the manifests in this order:

```bash
talon-cli apply -f manifests/examples/medicalscribe-template-editor/namespace-production.yaml
talon-cli apply -f manifests/examples/medicalscribe-template-editor/namespace-staging.yaml
talon-cli apply -f manifests/examples/medicalscribe-template-editor/template.yaml
talon-cli apply -f manifests/examples/medicalscribe-template-editor/deployment.yaml
```

The resulting agents use Talon sessions and streaming for the conversation.
Medical Scribe remains responsible for user authorization, template
ownership, preview rendering, and saving an approved template.

The proposal block is a temporary text-level contract for the current
interactive session API. It can be replaced by Talon's structured-output
session support without changing the Agent or Deployment shape.
