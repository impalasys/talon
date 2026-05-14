fn main() -> std::io::Result<()> {
    // All message types in the manifests package get serde support so they can
    // be deserialized directly from YAML/JSON without shadow structs.
    // The rename_all = "camelCase" matches the YAML convention (systemPrompt, apiVersion, etc.)
    let serde_default_types = [
        ".talon.events.SessionStepEvent",
        ".talon.manifests.FeatureSetDelta",
        ".talon.manifests.McpAuthBrokerSpec",
        ".talon.manifests.McpServer",
        ".talon.manifests.McpServerSpec",
        ".talon.manifests.McpServerBinding",
        ".talon.manifests.McpServerBindingSpec",
        ".talon.manifests.Knowledge",
        ".talon.manifests.KnowledgeSpec",
        ".talon.manifests.ObjectMeta",
        ".talon.manifests.Model",
        ".talon.manifests.ModelProfile",
        ".talon.manifests.ModelPolicy",
        ".talon.manifests.ModelPolicyDelta",
        ".talon.manifests.SchedulePolicy",
        ".talon.manifests.SchedulePolicyDelta",
        ".talon.manifests.Feature",
        ".talon.manifests.StringListDelta",
        ".talon.models.Namespace",
        ".talon.models.Schedule",
        ".talon.models.ScheduleEvent",
        ".talon.models.ScheduleSpec",
        ".talon.models.ScheduleStatus",
        ".talon.models.ScheduleTarget",
        ".talon.models.Session",
        ".talon.models.SessionMessage",
    ];
    let serde_derive_only_types = [
        ".talon.manifests.PromptDelta",
    ];

    let mut builder = tonic_build::configure();
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
            "proto/models.proto",
            "proto/manifests.proto",
            "proto/gateway.proto",
            "proto/events.proto",
        ],
        &[".", "third_party/googleapis/"],
    )?;

    Ok(())
}
