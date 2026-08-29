use super::*;


pub(crate) async fn list_files_tool(
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

pub(crate) async fn read_file_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    args: &Value,
) -> Result<ToolOutput> {
    let (namespace, path) = file_location_from_args(current_namespace, args)?;
    ensure_file_read_namespace(current_namespace, &namespace)?;
    let file = find_file_by_path(cp, &namespace, &path)
        .await?
        .ok_or_else(|| anyhow!("File '{}' not found", path))?;
    read_file_output(cp, &file).await
}

pub(crate) async fn get_file_metadata_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    args: &Value,
) -> Result<String> {
    let (namespace, path) = file_location_from_args(current_namespace, args)?;
    ensure_file_read_namespace(current_namespace, &namespace)?;
    let file = find_file_by_path(cp, &namespace, &path)
        .await?
        .ok_or_else(|| anyhow!("File '{}' not found", path))?;
    Ok(serde_json::to_string_pretty(&file_json(&file, false))?)
}

pub(crate) async fn create_file_tool(
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

pub(crate) async fn update_file_tool(
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

pub(crate) async fn delete_file_tool(
    cp: &ControlPlane,
    current_namespace: &str,
    args: &Value,
) -> Result<String> {
    let (namespace, path) = file_location_from_args(current_namespace, args)?;
    let file = find_file_by_path(cp, &namespace, &path)
        .await?
        .ok_or_else(|| anyhow!("File '{}' not found", path))?;
    let name = file_name_from_file(&file);
    let latest_key = crate::control::cas::latest_file_object_key(&namespace, &path);
    let object_key = file
        .status
        .as_ref()
        .and_then(|status| status.object_ref.as_ref())
        .map(|object| object.key.clone());
    let store = ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    let deleted = store.delete(&namespace, "File", &name).await?;
    if deleted {
        let cas = crate::control::cas::CasStore::new(cp.objects.clone());
        if let Err(error) = cas.delete_object(&latest_key).await {
            tracing::warn!(
                error = %error,
                object_key = %latest_key,
                "failed to delete latest File object from native tool"
            );
        }
        if let Some(object_key) = object_key {
            if let Err(error) = cas.delete_object(&object_key).await {
                tracing::warn!(
                    error = %error,
                    object_key = %object_key,
                    "failed to delete File CAS object from native tool"
                );
            }
        }
    }
    Ok(serde_json::to_string_pretty(&json!({
        "deleted": deleted,
        "namespace": namespace,
        "path": path,
    }))?)
}

pub(crate) async fn list_files_by_filter(
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

pub(crate) async fn read_file_content(cp: &ControlPlane, file: &resources_proto::File) -> Result<String> {
    let read_object = read_file_object(cp, file).await?;
    Ok(String::from_utf8_lossy(&read_object.object.bytes).to_string())
}

pub(crate) async fn read_file_output(cp: &ControlPlane, file: &resources_proto::File) -> Result<ToolOutput> {
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

pub(crate) async fn read_file_object(
    cp: &ControlPlane,
    file: &resources_proto::File,
) -> Result<ReadFileObject> {
    let object_ref = file
        .status
        .as_ref()
        .and_then(|status| status.object_ref.as_ref())
        .ok_or_else(|| anyhow!("File has no objectRef"))?;
    let object = crate::control::cas::CasStore::new(cp.objects.clone())
        .get_object_decoded(&object_ref.key)
        .await?
        .ok_or_else(|| anyhow!("File object '{}' not found", object_ref.key))?;
    let object_ref = crate::control::cas::object_ref_from_stored_object(&object_ref.key, &object);
    Ok(ReadFileObject { object, object_ref })
}

pub(crate) async fn upsert_file(
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

pub(crate) async fn write_file_objects(
    cp: &ControlPlane,
    namespace: &str,
    file_uid: &str,
    path: &str,
    media_type: &str,
    content: &[u8],
) -> Result<resources_proto::FileObjectRef> {
    let cas = crate::control::cas::CasStore::new(cp.objects.clone());
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

pub(crate) fn file_from_resource(resource: resources_proto::Resource) -> Option<resources_proto::File> {
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

pub(crate) fn file_name_from_file(file: &resources_proto::File) -> String {
    file.metadata
        .as_ref()
        .map(|metadata| metadata.name.clone())
        .unwrap_or_default()
}

pub(crate) fn file_json(file: &resources_proto::File, include_resource_name: bool) -> Value {
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

pub(crate) fn file_uri(namespace: &str, path: &str) -> String {
    format!("file://{}{}", namespace, path)
}

pub(crate) fn file_location_from_args(current_namespace: &str, args: &Value) -> Result<(String, String)> {
    if let Some(uri) = opt_str(args, "uri") {
        return parse_file_uri(uri);
    }
    let namespace = namespace_arg(current_namespace, args);
    let path = normalize_logical_path(req_str(args, "path")?)?;
    Ok((namespace, path))
}

pub(crate) fn namespace_arg(current_namespace: &str, args: &Value) -> String {
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
    let requested_namespace = if requested_namespace == "current" {
        current_namespace
    } else {
        requested_namespace
    };
    if crate::control::ns::ancestry(current_namespace)
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

pub(crate) fn file_resource_labels(
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

pub(crate) fn file_purpose_label(value: i32) -> &'static str {
    match resources_proto::FilePurpose::try_from(value).ok() {
        Some(resources_proto::FilePurpose::Memory) => "memory",
        Some(resources_proto::FilePurpose::Artifact) => "artifact",
        Some(resources_proto::FilePurpose::Skill) => "skill",
        _ => "unspecified",
    }
}

pub(crate) fn file_index_policy_label(value: i32) -> &'static str {
    match resources_proto::FileIndexPolicy::try_from(value).ok() {
        Some(resources_proto::FileIndexPolicy::None) => "none",
        Some(resources_proto::FileIndexPolicy::Search) => "search",
        Some(resources_proto::FileIndexPolicy::Retrieval) => "retrieval",
        _ => "unspecified",
    }
}

pub(crate) fn file_retention_label(value: i32) -> &'static str {
    match resources_proto::FileRetention::try_from(value).ok() {
        Some(resources_proto::FileRetention::Retained) => "retained",
        _ => "unspecified",
    }
}

pub(crate) fn parse_file_purpose(value: &str) -> Result<i32> {
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

pub(crate) fn parse_file_index_policy(value: &str) -> Result<i32> {
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

pub(crate) fn parse_file_retention(value: &str) -> Result<i32> {
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

pub(crate) fn normalize_enum_input(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['_', ' '], "-")
}
