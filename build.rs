// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

fn main() -> std::io::Result<()> {
    // All user-facing resource/data protobufs get serde support so they can
    // be deserialized directly from YAML/JSON without shadow structs.
    // The rename_all = "camelCase" matches the YAML convention (systemPrompt, apiVersion, etc.)
    let serde_default_types = [
        ".talon.resources.AcpRuntime",
        ".talon.resources.AgentCard",
        ".talon.resources.AgentCardCapabilities",
        ".talon.resources.AgentCardSkill",
        ".talon.resources.AgentRuntime",
        ".talon.resources.AgentStatus",
        ".talon.resources.Channel",
        ".talon.resources.ChannelContextPolicy",
        ".talon.resources.ChannelSpec",
        ".talon.resources.ChannelStatus",
        ".talon.resources.ChannelSubscription",
        ".talon.resources.ChannelSubscriptionSpec",
        ".talon.resources.CommonResourceStatus",
        ".talon.resources.ConnectionPoolPolicy",
        ".talon.resources.ConnectionSpec",
        ".talon.resources.ConnectionTransport",
        ".talon.resources.DeploymentPlacement",
        ".talon.resources.DeploymentReplicaSpec",
        ".talon.resources.DeploymentReplicaStatus",
        ".talon.resources.DeploymentSpec",
        ".talon.resources.DeploymentStatus",
        ".talon.resources.Feature",
        ".talon.resources.Knowledge",
        ".talon.resources.KnowledgeSpec",
        ".talon.resources.McpAuthBrokerSpec",
        ".talon.resources.McpServer",
        ".talon.resources.McpServerBinding",
        ".talon.resources.McpServerBindingSpec",
        ".talon.resources.McpServerSpec",
        ".talon.resources.Model",
        ".talon.resources.ModelPolicy",
        ".talon.resources.ModelProfile",
        ".talon.resources.Namespace",
        ".talon.resources.NamespaceSelector",
        ".talon.resources.NamespaceSpec",
        ".talon.resources.NamespaceStatus",
        ".talon.resources.OwnerReference",
        ".talon.resources.PermissionRequestSpec",
        ".talon.resources.PermissionRequestStatus",
        ".talon.resources.RawResourceSpec",
        ".talon.resources.RawResourceStatus",
        ".talon.resources.ResourceCondition",
        ".talon.resources.ResourceMeta",
        ".talon.resources.ResourceRef",
        ".talon.resources.Sandbox",
        ".talon.resources.SandboxClassSpec",
        ".talon.resources.SandboxFilesystemSpec",
        ".talon.resources.SandboxLease",
        ".talon.resources.SandboxLeasePolicySpec",
        ".talon.resources.SandboxNetworkSpec",
        ".talon.resources.SandboxPolicySpec",
        ".talon.resources.SandboxProcessStatus",
        ".talon.resources.SandboxSpec",
        ".talon.resources.SandboxStatus",
        ".talon.resources.SandboxRuntimeTemplateSpec",
        ".talon.resources.SandboxSetupSpec",
        ".talon.resources.SandboxWorkspaceSpec",
        ".talon.resources.Schedule",
        ".talon.resources.ScheduleEvent",
        ".talon.resources.ScheduleSpec",
        ".talon.resources.ScheduleStatus",
        ".talon.resources.ScheduleTarget",
        ".talon.resources.SessionSpec",
        ".talon.resources.SessionStatus",
        ".talon.resources.TemplateSpec",
        ".talon.resources.ThinkingConfig",
        ".talon.resources.Workflow",
        ".talon.resources.WorkflowSpec",
        ".talon.resources.WorkflowStatus",
        ".talon.resources.WorkflowStep",
        ".talon.resources.WorkflowStepOutputPolicy",
        ".talon.resources.WorkflowStepRetryPolicy",
        ".talon.data.ChannelMessage",
        ".talon.data.Knowledge",
        ".talon.data.KnowledgeSearchResult",
        ".talon.data.ObjectRef",
        ".talon.data.Session",
        ".talon.data.SessionMessage",
        ".talon.data.SessionMessagePart",
        ".talon.data.WorkflowRun",
        ".talon.data.WorkflowRunEvent",
        ".talon.data.WorkflowStepRun",
        ".talon.events.WorkflowDispatchEvent",
    ];
    let serde_derive_only_types: [&str; 0] = [];

    let mut builder = tonic_build::configure().protoc_arg("--experimental_allow_proto3_optional");
    for t in &serde_default_types {
        builder = builder
            .type_attribute(t, "#[derive(serde::Serialize, serde::Deserialize)]")
            .type_attribute(t, "#[serde(rename_all = \"camelCase\", default)]");
    }
    for t in &serde_derive_only_types {
        builder = builder.type_attribute(t, "#[derive(serde::Serialize, serde::Deserialize)]");
    }
    builder.compile_protos(
        &[
            "proto/config.proto",
            "proto/resources/common.proto",
            "proto/resources/agents.proto",
            "proto/resources/mcp.proto",
            "proto/resources/knowledge.proto",
            "proto/resources/namespaces.proto",
            "proto/resources/channels.proto",
            "proto/resources/schedules.proto",
            "proto/resources/workflows.proto",
            "proto/resources/deployments.proto",
            "proto/resources/sandboxes.proto",
            "proto/resources/sessions.proto",
            "proto/resources/resource.proto",
            "proto/data/data.proto",
            "proto/gateway.proto",
            "proto/events.proto",
        ],
        &[".", "third_party/googleapis/"],
    )?;

    Ok(())
}
