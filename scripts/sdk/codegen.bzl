"""Public SDK proto selection and Go package policy, shared by upstream rules."""

_GO_PACKAGES = {
    "proto/config.proto": "config",
    "proto/resources/common.proto": "resources",
    "proto/resources/agents.proto": "resources",
    "proto/resources/mcp.proto": "resources",
    "proto/resources/knowledge.proto": "resources",
    "proto/resources/namespaces.proto": "resources",
    "proto/resources/channels.proto": "resources",
    "proto/resources/connectors.proto": "resources",
    "proto/resources/files.proto": "resources",
    "proto/resources/schedules.proto": "resources",
    "proto/resources/secrets.proto": "resources",
    "proto/resources/workflows.proto": "resources",
    "proto/resources/deployments.proto": "resources",
    "proto/resources/sandboxes.proto": "resources",
    "proto/resources/sessions.proto": "resources",
    "proto/resources/skills.proto": "resources",
    "proto/resources/tasks.proto": "resources",
    "proto/resources/usage.proto": "resources",
    "proto/resources/workers.proto": "resources",
    "proto/resources/resource.proto": "resources",
    "proto/harness/llm.proto": "data",
    "proto/data/api_keys.proto": "data",
    "proto/data/connectors.proto": "routing",
    "proto/data/data.proto": "data",
    "proto/data/routing.proto": "routing",
    "proto/data/search.proto": "data",
    "proto/data/session_submission.proto": "data",
    "proto/data/session_journal_entry.proto": "data",
    "proto/events.proto": "events",
    "proto/external/connectors.proto": "external",
    "proto/talon/v1/auth.proto": "v1",
    "proto/talon/v1/cas.proto": "v1",
    "proto/talon/v1/channels.proto": "v1",
    "proto/talon/v1/connectors.proto": "v1",
    "proto/talon/v1/files.proto": "v1",
    "proto/talon/v1/knowledge.proto": "v1",
    "proto/talon/v1/namespaces.proto": "v1",
    "proto/talon/v1/resources.proto": "v1",
    "proto/talon/v1/search.proto": "v1",
    "proto/talon/v1/sessions.proto": "v1",
    "proto/talon/v1/workflows.proto": "v1",
}

SDK_PROTO_SOURCES = _GO_PACKAGES.keys()
_GO_MODULE = "github.com/impalasys/talon/sdk/go/talon-client"
GO_OPTIONS = ["module=" + _GO_MODULE] + [
    "M" + source + "=" + _GO_MODULE + "/talon/" + package
    for source, package in _GO_PACKAGES.items()
]
