// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use serde_json::{json, Value};

use crate::control::tool_output::ToolOutputExt;
use crate::control::ControlPlane;
use crate::harness::llm::ToolOutput;
use crate::harness::skills::registry::ToolRegistry;

pub(super) fn register(registry: &mut ToolRegistry) {
    registry.register_builtin(
        super::GET_ARTIFACT_METADATA_TOOL,
        "Return artifact metadata for an artifact:// URI without reading bytes.",
        json!({
            "type": "object",
            "properties": {
                "artifact_uri": { "type": "string" }
            },
            "required": ["artifact_uri"]
        }),
    );
    registry.register_builtin(
        super::GRANT_ARTIFACT_TOOL,
        "Grant another agent or session access to an artifact:// URI.",
        json!({
            "type": "object",
            "properties": {
                "artifact_uri": { "type": "string" },
                "target_agent": { "type": "string" },
                "target_session_id": { "type": "string" },
                "operations": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["read", "metadata", "promote"] }
                },
                "ttl_seconds": { "type": "integer" }
            },
            "required": ["artifact_uri"]
        }),
    );
}

pub(super) async fn execute_output(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    name: &str,
    args: &Value,
) -> Result<Option<ToolOutput>> {
    match name {
        // Kept as non-registered internal aliases while callers migrate to
        // generic read/write. They are never exposed to a model registry.
        super::CREATE_ARTIFACT_TOOL => {
            super::create_artifact(cp, current_namespace, current_agent, current_session, args)
                .await
                .map(ToolOutput::text)
                .map(Some)
        }
        super::READ_ARTIFACT_TOOL => {
            super::read_artifact(cp, current_namespace, current_agent, current_session, args)
                .await
                .map(Some)
        }
        super::UPDATE_ARTIFACT_TOOL => {
            super::update_artifact(cp, current_namespace, current_agent, current_session, args)
                .await
                .map(ToolOutput::text)
                .map(Some)
        }
        super::GET_ARTIFACT_METADATA_TOOL => super::get_artifact_metadata(
            cp,
            current_namespace,
            current_agent,
            current_session,
            args,
        )
        .await
        .map(ToolOutput::text)
        .map(Some),
        super::GRANT_ARTIFACT_TOOL => {
            super::grant_artifact(cp, current_namespace, current_agent, current_session, args)
                .await
                .map(ToolOutput::text)
                .map(Some)
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_no_top_level_composition(schema: &Value) {
        for keyword in ["anyOf", "oneOf", "allOf", "not"] {
            assert!(
                schema.get(keyword).is_none(),
                "OpenAI function schemas reject top-level {keyword}: {schema}"
            );
        }
    }

    #[test]
    fn artifact_tool_schemas_are_openai_compatible() {
        let mut registry = ToolRegistry::new();
        register(&mut registry);

        let tools = registry.list_tools();
        assert!(!tools.is_empty(), "artifact tools should be registered");
        for tool in tools {
            assert_eq!(
                tool.input_schema["type"], "object",
                "artifact tool {} input schema must be an object",
                tool.name
            );
            assert_no_top_level_composition(&tool.input_schema);
        }
    }
}
