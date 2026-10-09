// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Generic resource I/O dispatch.
//!
//! Add URI schemes to `RESOURCE_HANDLERS`; the generic tools deliberately do
//! not need a new top-level dispatch arm for every resource implementation.

use super::*;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResourceKind {
    File,
    Artifact,
}

struct ResourceHandler {
    scheme: &'static str,
    kind: ResourceKind,
}

// Extension point for future resource implementations (for example, tr://).
const RESOURCE_HANDLERS: &[ResourceHandler] = &[
    ResourceHandler {
        scheme: "file",
        kind: ResourceKind::File,
    },
    ResourceHandler {
        scheme: "artifact",
        kind: ResourceKind::Artifact,
    },
];

pub(crate) fn read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ref": { "type": "string", "description": "Resource URI to read, such as file://<namespace>/<path> or artifact://<namespace>/<agent>/<session>/<id>." }
        },
        "required": ["ref"]
    })
}

pub(crate) fn write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": { "type": "string", "enum": ["file", "artifact"], "description": "Required when creating a resource; optional for updates, where ref determines the kind." },
            "ref": { "type": "string", "description": "Existing resource URI to update." },
            "uri": { "type": "string", "description": "file:// URI for creating a File when path is not used." },
            "namespace": { "type": "string", "description": "File namespace. Defaults to the current namespace." },
            "path": { "type": "string", "description": "Logical File path when creating a File." },
            "title": { "type": "string", "description": "Human-readable title when creating an artifact." },
            "content": { "type": "string", "description": "Text content to store. Required unless content_base64 is supplied for an artifact." },
            "content_base64": { "type": "string", "description": "Base64 bytes to store for an artifact." },
            "media_type": { "type": "string", "description": "Optional media type." },
            "purpose": { "type": "string", "description": "File purpose: ARTIFACT, MEMORY, or SKILL." },
            "index_policy": { "type": "string", "description": "File index policy: NONE, SEARCH, or RETRIEVAL." },
            "retention": { "type": "string", "description": "File retention policy: RETAINED." },
            "labels": { "type": "object", "description": "Artifact labels.", "additionalProperties": { "type": "string" } },
            "metadata": { "type": "object", "description": "Artifact metadata.", "additionalProperties": { "type": "string" } }
        }
    })
}

pub(crate) async fn read_resource(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    args: &Value,
    config: &Config,
) -> Result<ToolOutput> {
    let reference = req_str(args, "ref")?;
    match handler_for_uri(reference)?.kind {
        ResourceKind::File => {
            require_global_file_capability(config, "read")?;
            require_file_read(spec)?;
            files::read_file_tool(cp, current_namespace, &args_with(args, "uri", reference)?).await
        }
        ResourceKind::Artifact => {
            artifacts::read_artifact(
                cp,
                current_namespace,
                current_agent,
                current_session,
                &args_with(args, "artifact_uri", reference)?,
            )
            .await
        }
    }
}

pub(crate) async fn write_resource(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    args: &Value,
    config: &Config,
) -> Result<String> {
    if let Some(reference) = opt_str(args, "ref") {
        let handler = handler_for_uri(reference)?;
        ensure_kind_matches(args, handler.kind)?;
        return match handler.kind {
            ResourceKind::File => {
                require_global_file_capability(config, "update")?;
                require_capability(spec, "files", "update")?;
                files::update_file_tool(cp, current_namespace, &args_with(args, "uri", reference)?)
                    .await
            }
            ResourceKind::Artifact => {
                artifacts::update_artifact(
                    cp,
                    current_namespace,
                    current_agent,
                    current_session,
                    &args_with(args, "artifact_uri", reference)?,
                )
                .await
            }
        };
    }

    match kind_from_args(args)? {
        ResourceKind::File => {
            require_global_file_capability(config, "create")?;
            require_capability(spec, "files", "create")?;
            files::create_file_tool(cp, current_namespace, args).await
        }
        ResourceKind::Artifact => {
            artifacts::create_artifact(cp, current_namespace, current_agent, current_session, args)
                .await
        }
    }
}

fn handler_for_uri(reference: &str) -> Result<&'static ResourceHandler> {
    let scheme = reference
        .trim()
        .split_once("://")
        .map(|(scheme, _)| scheme)
        .filter(|scheme| !scheme.is_empty())
        .ok_or_else(|| anyhow!("resource ref must include a URI scheme"))?;
    RESOURCE_HANDLERS
        .iter()
        .find(|handler| handler.scheme == scheme)
        .ok_or_else(|| anyhow!("unsupported resource URI scheme '{scheme}'"))
}

fn kind_from_args(args: &Value) -> Result<ResourceKind> {
    match req_str(args, "kind")? {
        "file" => Ok(ResourceKind::File),
        "artifact" => Ok(ResourceKind::Artifact),
        other => Err(anyhow!("unsupported resource kind '{other}'")),
    }
}

fn ensure_kind_matches(args: &Value, kind: ResourceKind) -> Result<()> {
    let Some(requested) = opt_str(args, "kind") else {
        return Ok(());
    };
    let requested = match requested {
        "file" => ResourceKind::File,
        "artifact" => ResourceKind::Artifact,
        other => return Err(anyhow!("unsupported resource kind '{other}'")),
    };
    if requested == kind {
        Ok(())
    } else {
        Err(anyhow!("resource kind does not match ref URI scheme"))
    }
}

fn args_with(args: &Value, key: &str, value: &str) -> Result<Value> {
    let mut args = args.clone();
    args.as_object_mut()
        .ok_or_else(|| anyhow!("tool arguments must be a JSON object"))?
        .insert(key.to_string(), Value::String(value.to_string()));
    Ok(args)
}

fn require_global_file_capability(config: &Config, action: &str) -> Result<()> {
    if global_capability_allowed(config, "files", action) {
        Ok(())
    } else {
        Err(anyhow!(
            "capability 'files:{action}' is disabled by deployment configuration"
        ))
    }
}
