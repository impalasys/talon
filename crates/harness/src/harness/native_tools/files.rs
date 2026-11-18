// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[allow(unused_imports)]
use crate::talon;

use super::*;

pub async fn list_files_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    args: &Value,
) -> Result<String> {
    let namespace = namespace_arg(current_namespace, args);
    let prefix = opt_str(args, "prefix")
        .map(normalize_logical_path)
        .transpose()?;
    let purpose = opt_str(args, "purpose")
        .map(parse_file_purpose)
        .transpose()?;
    let index_policy = opt_str(args, "index_policy")
        .map(parse_file_index_policy)
        .transpose()?;
    let limit = opt_usize(args, "limit").unwrap_or(50).clamp(1, 100);
    let entries = list_files_by_filter(
        cp,
        &namespace,
        prefix.as_deref().unwrap_or(""),
        purpose,
        index_policy,
    )
    .await?
    .into_iter()
    .take(limit)
    .map(|file| file_json(&file, false))
    .collect::<Vec<_>>();
    Ok(serde_json::to_string_pretty(
        &json!({ "entries": entries }),
    )?)
}

pub(super) async fn read_authorized_file_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    args: &Value,
) -> Result<ToolOutput> {
    let (namespace, path) = file_location_from_args(current_namespace, args)?;
    let file = find_file_by_path(cp, &namespace, &path)
        .await?
        .ok_or_else(|| anyhow!("File '{}' not found", path))?;
    read_file_output(cp, &file).await
}

pub(super) async fn get_authorized_file_metadata_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    args: &Value,
) -> Result<String> {
    let (namespace, path) = file_location_from_args(current_namespace, args)?;
    let file = find_file_by_path(cp, &namespace, &path)
        .await?
        .ok_or_else(|| anyhow!("File '{}' not found", path))?;
    Ok(serde_json::to_string_pretty(&file_json(&file, false))?)
}

pub async fn create_file_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    args: &Value,
) -> Result<String> {
    let (namespace, path) = file_location_from_args(current_namespace, args)?;
    if find_file_by_path(cp, &namespace, &path).await?.is_some() {
        return Err(anyhow!("File '{}' already exists", path));
    }
    let content = req_str(args, "content")?;
    let media_type = opt_str(args, "media_type").unwrap_or("text/markdown");
    let purpose = opt_str(args, "purpose")
        .map(parse_file_purpose)
        .transpose()?
        .unwrap_or(resources_proto::FilePurpose::Artifact as i32);
    let index_policy = opt_str(args, "index_policy")
        .map(parse_file_index_policy)
        .transpose()?
        .unwrap_or(resources_proto::FileIndexPolicy::Search as i32);
    let retention = opt_str(args, "retention")
        .map(parse_file_retention)
        .transpose()?
        .unwrap_or(resources_proto::FileRetention::Retained as i32);
    let file = upsert_file(
        cp,
        &namespace,
        None,
        &path,
        media_type,
        purpose,
        index_policy,
        retention,
        content.as_bytes(),
    )
    .await?;
    Ok(serde_json::to_string_pretty(&json!({
        "file": file_json(&file, false),
    }))?)
}

pub async fn update_file_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    args: &Value,
) -> Result<String> {
    let (namespace, path) = file_location_from_args(current_namespace, args)?;
    let existing = find_file_by_path(cp, &namespace, &path)
        .await?
        .ok_or_else(|| anyhow!("File '{}' not found", path))?;
    let content = req_str(args, "content")?;
    let existing_spec = existing.spec.as_ref();
    let media_type = opt_str(args, "media_type")
        .or_else(|| existing_spec.map(|spec| spec.media_type.as_str()))
        .unwrap_or("text/markdown")
        .to_string();
    let purpose = opt_str(args, "purpose")
        .map(parse_file_purpose)
        .transpose()?
        .or_else(|| existing_spec.map(|spec| spec.purpose))
        .unwrap_or(resources_proto::FilePurpose::Artifact as i32);
    let index_policy = opt_str(args, "index_policy")
        .map(parse_file_index_policy)
        .transpose()?
        .or_else(|| existing_spec.map(|spec| spec.index_policy))
        .unwrap_or(resources_proto::FileIndexPolicy::Search as i32);
    let retention = opt_str(args, "retention")
        .map(parse_file_retention)
        .transpose()?
        .or_else(|| existing_spec.map(|spec| spec.retention))
        .unwrap_or(resources_proto::FileRetention::Retained as i32);
    let file = upsert_file(
        cp,
        &namespace,
        Some(existing),
        &path,
        &media_type,
        purpose,
        index_policy,
        retention,
        content.as_bytes(),
    )
    .await?;
    Ok(serde_json::to_string_pretty(&json!({
        "file": file_json(&file, false),
    }))?)
}

pub async fn delete_file_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<String> {
    let (namespace, path) = file_location_from_args(current_namespace, args)?;
    let file = find_file_by_path(cp, &namespace, &path)
        .await?
        .ok_or_else(|| anyhow!("File '{}' not found", path))?;
    if let Some(response) = require_delete_confirmation(
        cp,
        current_namespace,
        current_agent,
        current_session,
        args,
        &namespace,
        &path,
        &file,
    )
    .await?
    {
        return Ok(response);
    }
    let name = file_name_from_file(&file);
    let version = &file
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow!("File metadata missing"))?
        .resource_version;
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let deleted = store
        .delete_if_version(&namespace, "File", &name, version)
        .await?;
    // Content-addressed blobs can be shared by readers, and the mutable latest
    // alias can already belong to a concurrent replacement. Leave reclamation
    // to storage retention rather than deleting either after this metadata CAS.
    Ok(serde_json::to_string_pretty(&json!({
        "deleted": deleted, "namespace": namespace, "path": path,
    }))?)
}

const DELETE_CONFIRMATION_TTL_MICROS: i64 = 60 * 60 * 1_000_000;
#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct PendingFileDelete {
    target_namespace: String,
    path: String,
    resource_name: String,
    resource_version: String,
    requested_at: i64,
    expires_at: i64,
    required_reply: String,
}

async fn require_delete_confirmation(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
    target_namespace: &str,
    path: &str,
    file: &resources_proto::File,
) -> Result<Option<String>> {
    if current_session.trim().is_empty() {
        return Err(anyhow!(
            "file deletion requires a session so Talon can verify a later user confirmation"
        ));
    }
    let resource_name = file_name_from_file(file);
    let resource_version = file
        .metadata
        .as_ref()
        .map(|meta| meta.resource_version.clone())
        .unwrap_or_default();

    let Some(confirmation_id) = opt_str(args, "confirmation_id") else {
        let now = chrono::Utc::now().timestamp_micros();
        let confirmation_id = talon::control::uuid::session_message_id();
        let required_reply = format!(
            "CONFIRM DELETE FILE {} [{}]",
            file_uri(target_namespace, path),
            confirmation_id
        );
        let pending = PendingFileDelete {
            target_namespace: target_namespace.to_string(),
            path: path.to_string(),
            resource_name,
            resource_version,
            requested_at: now,
            expires_at: now + DELETE_CONFIRMATION_TTL_MICROS,
            required_reply: required_reply.clone(),
        };
        cp.kv
            .set(
                &delete_confirmation_key(
                    current_namespace,
                    current_agent,
                    current_session,
                    &confirmation_id,
                ),
                &serde_json::to_vec(&pending)?,
            )
            .await?;
        return Ok(Some(serde_json::to_string_pretty(&json!({
            "status": "confirmation_required",
            "confirmationId": confirmation_id,
            "namespace": target_namespace,
            "path": path,
            "requiredReply": required_reply,
            "instruction": "Ask the user to send requiredReply exactly in a separate message. Do not call this delete tool again until that reply arrives."
        }))?));
    };

    let key = delete_confirmation_key(
        current_namespace,
        current_agent,
        current_session,
        confirmation_id,
    );
    let pending_bytes = cp.kv.get(&key).await?.ok_or_else(|| {
        anyhow!(
            "delete confirmation '{}' is missing or already used",
            confirmation_id
        )
    })?;
    if pending_bytes == b"consumed" {
        return Err(anyhow!("delete confirmation was already used"));
    }
    let pending: PendingFileDelete = serde_json::from_slice(&pending_bytes)?;
    let now = chrono::Utc::now().timestamp_micros();
    if now > pending.expires_at {
        cp.kv.delete(&key).await?;
        return Err(anyhow!(
            "delete confirmation '{}' expired; start a new delete request",
            confirmation_id
        ));
    }
    if pending.target_namespace != target_namespace
        || pending.path != path
        || pending.resource_name != resource_name
        || pending.resource_version != resource_version
    {
        return Err(anyhow!(
            "delete confirmation '{}' does not match the exact current namespace, path, or File version",
            confirmation_id
        ));
    }

    let latest_user = latest_user_message(cp, current_namespace, current_agent, current_session)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "delete requires a separate user message exactly: {}",
                pending.required_reply
            )
        })?;
    let latest_text = latest_user
        .parts
        .iter()
        .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
        .map(|part| part.content.as_str())
        .collect::<String>();
    if latest_user.created_at <= pending.requested_at
        || latest_text.trim() != pending.required_reply
    {
        return Err(anyhow!(
            "delete requires a separate user message exactly: {}",
            pending.required_reply
        ));
    }
    if !cp
        .kv
        .compare_and_swap(&key, Some(&pending_bytes), b"consumed")
        .await?
    {
        return Err(anyhow!(
            "delete confirmation '{}' was already used",
            confirmation_id
        ));
    }
    Ok(None)
}

fn delete_confirmation_key(
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    confirmation_id: &str,
) -> keys::ResourceKey {
    keys::ResourceKey::new(
        current_namespace,
        &[("Agent", current_agent), ("Session", current_session)],
        "DeleteConfirmation",
        confirmation_id,
    )
}

async fn latest_user_message(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
) -> Result<Option<data_proto::SessionMessage>> {
    let entries = cp
        .kv
        .list_entries(
            &keys::session_message_prefix(current_namespace, current_agent, current_session),
            None,
        )
        .await?;
    Ok(entries
        .into_iter()
        .filter_map(|(_, bytes)| data_proto::SessionMessage::decode(bytes.as_slice()).ok())
        .filter(|message| message.role == data_proto::MessageRole::RoleUser as i32)
        .max_by_key(|message| message.created_at))
}

pub async fn list_files_by_filter(
    cp: &ControlPlane,
    namespace: &str,
    prefix: &str,
    purpose: Option<i32>,
    index_policy: Option<i32>,
) -> Result<Vec<resources_proto::File>> {
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let mut files = Vec::new();
    for resource in store.list(namespace, Some("File")).await? {
        let Some(file) = file_from_resource(resource) else {
            continue;
        };
        let Some(spec) = file.spec.as_ref() else {
            continue;
        };
        if purpose.is_some_and(|purpose| spec.purpose != purpose) {
            continue;
        }
        if index_policy.is_some_and(|index_policy| spec.index_policy != index_policy) {
            continue;
        }
        if !prefix.is_empty() && !spec.path.starts_with(prefix) {
            continue;
        }
        files.push(file);
    }
    files.sort_by(|left, right| {
        left.spec
            .as_ref()
            .map(|spec| spec.path.as_str())
            .cmp(&right.spec.as_ref().map(|spec| spec.path.as_str()))
    });
    Ok(files)
}

pub async fn find_file_by_path(
    cp: &ControlPlane,
    namespace: &str,
    path: &str,
) -> Result<Option<resources_proto::File>> {
    Ok(list_files_by_filter(cp, namespace, path, None, None)
        .await?
        .into_iter()
        .find(|file| file.spec.as_ref().map(|spec| spec.path.as_str()) == Some(path)))
}

pub async fn read_file_content(cp: &ControlPlane, file: &resources_proto::File) -> Result<String> {
    let read_object = read_file_object(cp, file).await?;
    Ok(String::from_utf8_lossy(&read_object.object.bytes).to_string())
}

pub async fn read_file_output(
    cp: &ControlPlane,
    file: &resources_proto::File,
) -> Result<ToolOutput> {
    let read_object = read_file_object(cp, file).await?;
    let spec_media_type = file
        .spec
        .as_ref()
        .map(|spec| spec.media_type.trim())
        .unwrap_or_default();
    let object_media_type = read_object.object.metadata.media_type.trim();
    let media_type = if !spec_media_type.is_empty() {
        spec_media_type.to_string()
    } else if !object_media_type.is_empty() {
        object_media_type.to_string()
    } else {
        file.spec
            .as_ref()
            .and_then(|spec| mime_guess::from_path(&spec.path).first_raw())
            .unwrap_or("application/octet-stream")
            .to_string()
    };
    let filename = {
        let metadata_filename = read_object.object.metadata.filename.trim();
        if !metadata_filename.is_empty() {
            metadata_filename.to_string()
        } else {
            file.spec
                .as_ref()
                .map(|spec| {
                    spec.path
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or_default()
                        .to_string()
                })
                .unwrap_or_default()
        }
    };
    let mut object_ref = read_object.object_ref;
    object_ref.media_type = media_type.clone();
    object_ref.filename = filename.clone();
    Ok(ToolOutput::from_source_object(
        read_object.object.bytes,
        media_type,
        filename,
        object_ref,
    ))
}

pub async fn read_file_object(
    cp: &ControlPlane,
    file: &resources_proto::File,
) -> Result<ReadFileObject> {
    let object_ref = file
        .status
        .as_ref()
        .and_then(|status| status.object_ref.as_ref())
        .ok_or_else(|| anyhow!("File has no objectRef"))?;
    let object = talon::control::cas::CasStore::new(cp.objects.clone())
        .get_object_decoded(&object_ref.key)
        .await?
        .ok_or_else(|| anyhow!("File object '{}' not found", object_ref.key))?;
    let object_ref = talon::control::cas::object_ref_from_stored_object(&object_ref.key, &object);
    Ok(ReadFileObject { object, object_ref })
}

pub async fn upsert_file(
    cp: &ControlPlane,
    namespace: &str,
    existing: Option<resources_proto::File>,
    path: &str,
    media_type: &str,
    purpose: i32,
    index_policy: i32,
    retention: i32,
    content: &[u8],
) -> Result<resources_proto::File> {
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let name = existing
        .as_ref()
        .map(file_name_from_file)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| keys::file_name_for_path(path));
    let status = existing
        .as_ref()
        .and_then(|file| file.status.clone())
        .unwrap_or_default();
    let spec = resources_proto::FileSpec {
        path: path.to_string(),
        media_type: media_type.to_string(),
        purpose,
        index_policy,
        retention,
    };
    let mut resource = store
        .upsert(
            namespace,
            resource_model::file_resource(
                namespace.to_string(),
                name,
                spec,
                status,
                file_resource_labels(purpose, index_policy, retention),
            ),
        )
        .await?;
    let uid = resource
        .metadata
        .as_ref()
        .map(|metadata| metadata.uid.as_str())
        .filter(|uid| !uid.is_empty())
        .ok_or_else(|| anyhow!("File resource uid missing after upsert"))?;
    let object_ref = write_file_objects(cp, namespace, uid, path, media_type, content).await?;
    let status = resources_proto::FileStatus {
        observed_generation: resource
            .metadata
            .as_ref()
            .map(|metadata| metadata.generation)
            .unwrap_or_default(),
        phase: "Ready".to_string(),
        conditions: Vec::new(),
        object_ref: Some(object_ref),
        updated_at: chrono::Utc::now().timestamp_micros(),
        pending_upload: None,
    };
    resource.status = Some(resources_proto::ResourceStatus {
        kind: Some(resources_proto::resource_status::Kind::File(status)),
    });
    let name = resource
        .metadata
        .as_ref()
        .map(|metadata| metadata.name.clone())
        .ok_or_else(|| anyhow!("File resource name missing"))?;
    let resource = store
        .patch_status(namespace, "File", &name, None, resource.status.unwrap())
        .await?;
    file_from_resource(resource).ok_or_else(|| anyhow!("invalid File resource"))
}

pub async fn write_file_objects(
    cp: &ControlPlane,
    namespace: &str,
    file_uid: &str,
    path: &str,
    media_type: &str,
    content: &[u8],
) -> Result<resources_proto::FileObjectRef> {
    let cas = talon::control::cas::CasStore::new(cp.objects.clone());
    let object_ref = cas
        .put_file(namespace, file_uid, path, content, media_type)
        .await?;
    cas.put_latest_file(namespace, path, content, media_type)
        .await?;
    Ok(resources_proto::FileObjectRef {
        key: object_ref.key,
        media_type: object_ref.media_type,
        size_bytes: object_ref.size_bytes,
        sha256: object_ref.sha256,
        filename: object_ref.filename,
        metadata: object_ref.metadata,
    })
}

pub fn file_from_resource(resource: resources_proto::Resource) -> Option<resources_proto::File> {
    let spec = resource.spec.and_then(|spec| match spec.kind {
        Some(resources_proto::resource_spec::Kind::File(spec)) => Some(spec),
        _ => None,
    })?;
    let status = resource.status.and_then(|status| match status.kind {
        Some(resources_proto::resource_status::Kind::File(status)) => Some(status),
        _ => None,
    });
    Some(resources_proto::File {
        metadata: resource.metadata,
        spec: Some(spec),
        status,
    })
}

pub fn file_name_from_file(file: &resources_proto::File) -> String {
    file.metadata
        .as_ref()
        .map(|metadata| metadata.name.clone())
        .unwrap_or_default()
}

pub fn file_json(file: &resources_proto::File, include_resource_name: bool) -> Value {
    let namespace = file
        .metadata
        .as_ref()
        .map(|metadata| metadata.namespace.as_str())
        .unwrap_or_default();
    let spec = file.spec.as_ref();
    let path = spec.map(|spec| spec.path.as_str()).unwrap_or_default();
    let object = file
        .status
        .as_ref()
        .and_then(|status| status.object_ref.as_ref());
    let mut value = json!({
        "namespace": namespace,
        "uri": file_uri(namespace, path),
        "path": path,
        "mediaType": spec.map(|spec| spec.media_type.as_str()).unwrap_or_default(),
        "purpose": spec.map(|spec| file_purpose_label(spec.purpose)).unwrap_or("unspecified"),
        "indexPolicy": spec.map(|spec| file_index_policy_label(spec.index_policy)).unwrap_or("unspecified"),
        "retention": spec.map(|spec| file_retention_label(spec.retention)).unwrap_or("unspecified"),
        "sizeBytes": object.map(|object| object.size_bytes).unwrap_or_default(),
        "sha256": object.map(|object| object.sha256.as_str()).unwrap_or_default(),
    });
    if include_resource_name {
        if let Some(map) = value.as_object_mut() {
            map.insert("name".to_string(), json!(file_name_from_file(file)));
        }
    }
    value
}

pub fn file_uri(namespace: &str, path: &str) -> String {
    format!("file://{}{}", namespace, path)
}

pub fn file_location_from_args(current_namespace: &str, args: &Value) -> Result<(String, String)> {
    if let Some(uri) = opt_str(args, "uri") {
        return parse_file_uri(uri);
    }
    let namespace = namespace_arg(current_namespace, args);
    let path = normalize_logical_path(req_str(args, "path")?)?;
    Ok((namespace, path))
}

pub fn namespace_arg(current_namespace: &str, args: &Value) -> String {
    match opt_str(args, "namespace") {
        Some("current" | "current_namespace" | "current namespace") | None => {
            current_namespace.to_string()
        }
        Some(namespace) => namespace.to_string(),
    }
}

pub fn parse_file_uri(uri: &str) -> Result<(String, String)> {
    let rest = uri
        .trim()
        .strip_prefix("file://")
        .ok_or_else(|| anyhow!("file uri must start with 'file://'"))?;
    let split = rest
        .find('/')
        .ok_or_else(|| anyhow!("file uri must include namespace and path"))?;
    let namespace = rest[..split].trim();
    if namespace.is_empty() {
        return Err(anyhow!("file uri namespace is required"));
    }
    let path = normalize_logical_path(&rest[split..])?;
    Ok((namespace.to_string(), path))
}

pub fn ensure_file_read_namespace(
    current_namespace: &str,
    requested_namespace: &str,
) -> Result<()> {
    // This helper authorizes concrete namespaces. Argument aliases must be
    // resolved by namespace_arg before lookup; URI authorities are literal.
    if talon::control::ns::ancestry(current_namespace)
        .iter()
        .any(|namespace| namespace == requested_namespace)
    {
        Ok(())
    } else {
        Err(anyhow!(
            "file access to namespace '{}' is outside the current namespace ancestry",
            requested_namespace
        ))
    }
}

pub fn file_resource_labels(
    purpose: i32,
    index_policy: i32,
    retention: i32,
) -> HashMap<String, String> {
    HashMap::from([
        (
            "talon.impalasys.com/purpose".to_string(),
            file_purpose_label(purpose).to_string(),
        ),
        (
            "talon.impalasys.com/index-policy".to_string(),
            file_index_policy_label(index_policy).to_string(),
        ),
        (
            "talon.impalasys.com/retention".to_string(),
            file_retention_label(retention).to_string(),
        ),
    ])
}

pub fn file_purpose_label(value: i32) -> &'static str {
    match resources_proto::FilePurpose::try_from(value).ok() {
        Some(resources_proto::FilePurpose::Memory) => "memory",
        Some(resources_proto::FilePurpose::Artifact) => "artifact",
        Some(resources_proto::FilePurpose::Skill) => "skill",
        _ => "unspecified",
    }
}

pub fn file_index_policy_label(value: i32) -> &'static str {
    match resources_proto::FileIndexPolicy::try_from(value).ok() {
        Some(resources_proto::FileIndexPolicy::None) => "none",
        Some(resources_proto::FileIndexPolicy::Search) => "search",
        Some(resources_proto::FileIndexPolicy::Retrieval) => "retrieval",
        _ => "unspecified",
    }
}

pub fn file_retention_label(value: i32) -> &'static str {
    match resources_proto::FileRetention::try_from(value).ok() {
        Some(resources_proto::FileRetention::Retained) => "retained",
        _ => "unspecified",
    }
}

pub fn parse_file_purpose(value: &str) -> Result<i32> {
    let normalized = normalize_enum_input(value);
    match normalized.as_str() {
        "artifact" | "file-purpose-artifact" => Ok(resources_proto::FilePurpose::Artifact as i32),
        "memory" | "file-purpose-memory" => Ok(resources_proto::FilePurpose::Memory as i32),
        "skill" | "file-purpose-skill" => Ok(resources_proto::FilePurpose::Skill as i32),
        _ => Err(anyhow!(
            "unsupported File purpose '{}'; expected ARTIFACT, MEMORY, or SKILL",
            value
        )),
    }
}

pub fn parse_file_index_policy(value: &str) -> Result<i32> {
    let normalized = normalize_enum_input(value);
    match normalized.as_str() {
        "none" | "file-index-policy-none" => Ok(resources_proto::FileIndexPolicy::None as i32),
        "search" | "file-index-policy-search" => {
            Ok(resources_proto::FileIndexPolicy::Search as i32)
        }
        "retrieval" | "file-index-policy-retrieval" => {
            Ok(resources_proto::FileIndexPolicy::Retrieval as i32)
        }
        _ => Err(anyhow!(
            "unsupported File index_policy '{}'; expected NONE, SEARCH, or RETRIEVAL",
            value
        )),
    }
}

pub fn parse_file_retention(value: &str) -> Result<i32> {
    let normalized = normalize_enum_input(value);
    match normalized.as_str() {
        "retained" | "file-retention-retained" => {
            Ok(resources_proto::FileRetention::Retained as i32)
        }
        _ => Err(anyhow!(
            "unsupported File retention '{}'; expected RETAINED",
            value
        )),
    }
}

pub fn normalize_enum_input(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['_', ' '], "-")
}

/// Ancestry-scoped helper for internal callers without an AgentSpec.
pub async fn read_file_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    args: &Value,
) -> Result<ToolOutput> {
    let (namespace, _) = file_location_from_args(current_namespace, args)?;
    ensure_file_read_namespace(current_namespace, &namespace)?;
    read_authorized_file_tool(cp, current_namespace, args).await
}

/// Ancestry-scoped helper for internal callers without an AgentSpec.
pub async fn get_file_metadata_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    args: &Value,
) -> Result<String> {
    let (namespace, _) = file_location_from_args(current_namespace, args)?;
    ensure_file_read_namespace(current_namespace, &namespace)?;
    get_authorized_file_metadata_tool(cp, current_namespace, args).await
}

/// Reads retain namespace ancestry inheritance. Mutations require the current
/// namespace or an exact explicit grant; grants never imply an action permission.
pub(super) fn require_file_namespace_access(
    spec: &manifests::AgentSpec,
    current: &str,
    args: &Value,
    read: bool,
) -> Result<()> {
    let target = if opt_str(args, "uri").is_some() {
        file_location_from_args(current, args)?.0
    } else {
        namespace_arg(current, args)
    };
    require_file_namespace(spec, current, &target, read)
}

pub(super) fn require_file_list_namespace_access(
    spec: &manifests::AgentSpec,
    current: &str,
    args: &Value,
) -> Result<()> {
    require_file_namespace(spec, current, &namespace_arg(current, args), true)
}

fn require_file_namespace(
    spec: &manifests::AgentSpec,
    current: &str,
    target: &str,
    read: bool,
) -> Result<()> {
    if target == current
        || has_capability_action(spec, "fileNamespaces", target)
        || (read && ensure_file_read_namespace(current, target).is_ok())
    {
        Ok(())
    } else {
        Err(anyhow!("file namespace access denied for '{}'", target))
    }
}
