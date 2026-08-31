// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::dispatch::require_global_capability;
use super::*;

pub(crate) fn resource_read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ref": { "type": "string", "description": "file:// or artifact:// URI." },
            "namespace": { "type": "string", "description": "File namespace when reading by path. Defaults to the current namespace." },
            "path": { "type": "string", "description": "Logical File path when ref is omitted." }
        }
    })
}

pub(crate) fn resource_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ref": { "type": "string", "description": "Existing file:// or artifact:// URI to update." },
            "kind": { "type": "string", "enum": ["file", "artifact"], "description": "Required when ref is omitted to create a resource." },
            "namespace": { "type": "string", "description": "File namespace when creating a File. Defaults to the current namespace." },
            "path": { "type": "string", "description": "Required to create a File." },
            "title": { "type": "string", "description": "Required to create an Artifact." },
            "content": { "type": "string", "description": "Text content. Required unless content_base64 is provided." },
            "content_base64": { "type": "string", "description": "Base64 content for an Artifact." },
            "media_type": { "type": "string", "description": "Media type. Defaults to the existing or resource-specific default." },
            "purpose": { "type": "string", "description": "File purpose when creating or updating a File." },
            "index_policy": { "type": "string", "description": "File indexing policy when creating or updating a File." },
            "retention": { "type": "string", "description": "File retention when creating or updating a File." },
            "labels": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Artifact labels when creating an Artifact." },
            "metadata": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Artifact metadata when creating an Artifact." }
        }
    })
}

pub(crate) async fn read_resource_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    config: &Config,
    args: &Value,
) -> Result<ToolOutput> {
    let Some(reference) = opt_str(args, "ref") else {
        require_global_capability(config, "files", "read")?;
        require_file_read(spec)?;
        return read_file_tool(cp, current_namespace, args).await;
    };
    if reference.starts_with("artifact://") {
        return read_artifact(
            cp,
            current_namespace,
            current_agent,
            current_session,
            &args_with_string(args, "artifact_uri", reference)?,
        )
        .await;
    }
    if reference.starts_with("file://") {
        require_global_capability(config, "files", "read")?;
        require_file_read(spec)?;
        return read_file_tool(
            cp,
            current_namespace,
            &args_with_string(args, "uri", reference)?,
        )
        .await;
    }
    Err(anyhow!("read.ref must start with file:// or artifact://"))
}

pub(crate) async fn write_resource_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    spec: &manifests::AgentSpec,
    config: &Config,
    args: &Value,
) -> Result<String> {
    match opt_str(args, "ref") {
        Some(reference) if reference.starts_with("artifact://") => {
            update_artifact(
                cp,
                current_namespace,
                current_agent,
                current_session,
                &args_with_string(args, "artifact_uri", reference)?,
            )
            .await
        }
        Some(reference) if reference.starts_with("file://") => {
            require_global_capability(config, "files", "update")?;
            require_capability(spec, "files", "update")?;
            update_file_tool(
                cp,
                current_namespace,
                &args_with_string(args, "uri", reference)?,
            )
            .await
        }
        Some(_) => Err(anyhow!("write.ref must start with file:// or artifact://")),
        None => match req_str(args, "kind")? {
            "artifact" => {
                create_artifact(cp, current_namespace, current_agent, current_session, args).await
            }
            "file" => {
                require_global_capability(config, "files", "create")?;
                require_capability(spec, "files", "create")?;
                create_file_tool(cp, current_namespace, args).await
            }
            kind => Err(anyhow!(
                "unsupported write.kind '{kind}'; expected file or artifact"
            )),
        },
    }
}

fn args_with_string(args: &Value, key: &str, value: &str) -> Result<Value> {
    let mut args = args.clone();
    let object = args
        .as_object_mut()
        .ok_or_else(|| anyhow!("tool arguments must be an object"))?;
    object.insert(key.to_string(), Value::String(value.to_string()));
    Ok(args)
}
