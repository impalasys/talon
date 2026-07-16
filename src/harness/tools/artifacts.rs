// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use serde_json::{json, Value};

use crate::control::ControlPlane;
use crate::harness::skills::registry::ToolRegistry;

pub(super) fn register(registry: &mut ToolRegistry) {
    registry.register_builtin(
        super::CREATE_ARTIFACT_TOOL,
        "Create a session-scoped artifact and return an artifact:// URI that can be read or granted to another agent.",
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Human-readable artifact title." },
                "media_type": { "type": "string", "description": "Media type. Defaults to text/markdown." },
                "content": { "type": "string", "description": "Full text content to store. Required unless content_base64 is provided." },
                "content_base64": { "type": "string", "description": "Base64 bytes to store instead of content." },
                "labels": { "type": "object", "additionalProperties": { "type": "string" } },
                "metadata": { "type": "object", "additionalProperties": { "type": "string" } }
            },
            "required": ["title"]
        }),
    );
    registry.register_builtin(
        super::READ_ARTIFACT_TOOL,
        "Read an artifact by artifact:// URI.",
        json!({
            "type": "object",
            "properties": {
                "artifact_uri": { "type": "string" }
            },
            "required": ["artifact_uri"]
        }),
    );
    registry.register_builtin(
        super::UPDATE_ARTIFACT_TOOL,
        "Update an artifact owned by the current agent/session. Writes a new immutable object and keeps the same artifact:// URI.",
        json!({
            "type": "object",
            "properties": {
                "artifact_uri": { "type": "string" },
                "media_type": { "type": "string", "description": "Media type. Defaults to the artifact's current media type." },
                "content": { "type": "string", "description": "Full text content to store. Required unless content_base64 is provided." },
                "content_base64": { "type": "string", "description": "Base64 bytes to store instead of content." }
            },
            "required": ["artifact_uri"]
        }),
    );
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

pub(super) async fn execute(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    name: &str,
    args: &Value,
) -> Result<Option<String>> {
    match name {
        super::CREATE_ARTIFACT_TOOL => {
            super::create_artifact(cp, current_namespace, current_agent, current_session, args)
                .await
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
        .map(Some),
        super::GRANT_ARTIFACT_TOOL => {
            super::grant_artifact(cp, current_namespace, current_agent, current_session, args)
                .await
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

        let create = registry
            .get_tool(super::super::CREATE_ARTIFACT_TOOL)
            .expect("create_artifact should be registered");
        let update = registry
            .get_tool(super::super::UPDATE_ARTIFACT_TOOL)
            .expect("update_artifact should be registered");

        assert_eq!(create.input_schema["type"], "object");
        assert_eq!(update.input_schema["type"], "object");
        assert_no_top_level_composition(&create.input_schema);
        assert_no_top_level_composition(&update.input_schema);
    }
}
