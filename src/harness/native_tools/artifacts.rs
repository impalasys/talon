use super::*;
use crate::control::KeyValueStore;

pub async fn create_artifact(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<String> {
    if current_session.trim().is_empty() {
        return Err(anyhow!("create_artifact requires an active session"));
    }
    let title = req_str(args, "title")?;
    let media_type = opt_str(args, "media_type").unwrap_or("text/markdown");
    if args.get("content").and_then(Value::as_str).is_none()
        && opt_str(args, "content_base64").is_none()
    {
        return Err(anyhow!(
            "create_artifact requires content or content_base64"
        ));
    }
    let content = artifact_content_bytes(args)?;
    let labels = string_map(args.get("labels"));
    let metadata = string_map(args.get("metadata"));
    let artifact_id = crate::control::uuid::unique_name("artifact");
    let cas = crate::control::cas::CasStore::new(cp.objects.clone());
    let object_ref = cas
        .put_artifact(
            current_namespace,
            current_agent,
            current_session,
            &artifact_id,
            &content,
            media_type,
            metadata.clone(),
        )
        .await?;
    let artifact = crate::gateway::rpc::data_proto::Artifact {
        id: artifact_id.clone(),
        session_id: current_session.to_string(),
        title: title.to_string(),
        media_type: media_type.to_string(),
        object_ref: Some(object_ref),
        created_by_agent: current_agent.to_string(),
        created_at: chrono::Utc::now().timestamp_micros(),
        labels,
        metadata,
    };
    if let Err(error) = record_artifact_revision(
        cp.kv.as_ref(),
        current_namespace,
        current_agent,
        current_session,
        &artifact_id,
        artifact
            .object_ref
            .as_ref()
            .expect("new artifact object ref"),
    )
    .await
    {
        discard_uncommitted_artifact(
            &cas,
            cp.kv.as_ref(),
            current_namespace,
            current_agent,
            current_session,
            &artifact_id,
            artifact
                .object_ref
                .as_ref()
                .expect("new artifact object ref"),
            true,
            false,
        )
        .await;
        return Err(error);
    }
    if let Err(error) = cp
        .kv
        .set_msg(
            &keys::artifact(
                current_namespace,
                current_agent,
                current_session,
                &artifact_id,
            ),
            &artifact,
        )
        .await
    {
        discard_uncommitted_artifact(
            &cas,
            cp.kv.as_ref(),
            current_namespace,
            current_agent,
            current_session,
            &artifact_id,
            artifact
                .object_ref
                .as_ref()
                .expect("new artifact object ref"),
            true,
            true,
        )
        .await;
        return Err(error);
    }
    let artifact_uri = ArtifactUri {
        namespace: current_namespace.to_string(),
        agent: current_agent.to_string(),
        session_id: current_session.to_string(),
        artifact_id: artifact_id.clone(),
    }
    .encode();
    Ok(serde_json::to_string_pretty(&json!({
        "artifact": artifact_json(&artifact),
        "artifactUri": artifact_uri
    }))?)
}

pub async fn read_artifact(
    cp: &ControlPlane,
    _current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<ToolOutput> {
    let artifact_uri = req_str(args, "artifact_uri")?;
    let (_, artifact) =
        resolve_artifact_uri(cp, current_agent, current_session, artifact_uri, OP_READ).await?;
    let object_ref = artifact
        .object_ref
        .as_ref()
        .ok_or_else(|| anyhow!("Artifact has no objectRef"))?;
    let metadata = cp
        .objects
        .head(&object_ref.key)
        .await?
        .ok_or_else(|| anyhow!("Artifact object not found"))?;
    let media_type = if !artifact.media_type.trim().is_empty() {
        artifact.media_type.trim().to_string()
    } else if !object_ref.media_type.trim().is_empty() {
        object_ref.media_type.trim().to_string()
    } else if !metadata.media_type.trim().is_empty() {
        metadata.media_type.trim().to_string()
    } else {
        "application/octet-stream".to_string()
    };
    let filename = if !object_ref.filename.trim().is_empty() {
        object_ref.filename.clone()
    } else if !metadata.filename.trim().is_empty() {
        metadata.filename.clone()
    } else {
        artifact.title.clone()
    };
    let mut object_ref = crate::control::cas::object_ref_from_metadata(&object_ref.key, &metadata);
    object_ref.media_type = media_type.clone();
    object_ref.filename = filename.clone();
    if crate::control::tool_output::is_text_object_media_type(&media_type)
        && metadata.size_bytes
            < crate::control::tool_output::TOOL_RESULT_OBJECT_THRESHOLD_BYTES as u64
    {
        let object = crate::control::cas::CasStore::new(cp.objects.clone())
            .get_object_decoded(&object_ref.key)
            .await?
            .ok_or_else(|| anyhow!("Artifact object not found"))?;
        return Ok(ToolOutput::from_source_object(
            object.bytes,
            media_type,
            filename,
            object_ref,
        ));
    }
    Ok(ToolOutput::from_content_parts(
        vec![crate::harness::llm::object_ref_part(object_ref)],
        crate::control::tool_output::object_ref_summary(
            &media_type,
            &filename,
            metadata.size_bytes,
        ),
    ))
}

pub async fn update_artifact(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<String> {
    let artifact_uri = req_str(args, "artifact_uri")?;
    let uri = parse_artifact_uri(artifact_uri)?;
    if current_namespace != uri.namespace
        || current_agent != uri.agent
        || current_session != uri.session_id
    {
        return Err(anyhow!(
            "only the owning artifact namespace/agent/session may update '{artifact_uri}'"
        ));
    }
    let mut artifact = cp
        .kv
        .get_msg::<crate::gateway::rpc::data_proto::Artifact>(&keys::artifact(
            &uri.namespace,
            &uri.agent,
            &uri.session_id,
            &uri.artifact_id,
        ))
        .await?
        .ok_or_else(|| anyhow!("Artifact '{}' not found", uri.artifact_id))?;
    let previous_object_key = artifact
        .object_ref
        .as_ref()
        .map(|object_ref| object_ref.key.clone());
    let media_type = opt_str(args, "media_type").unwrap_or(&artifact.media_type);
    if args.get("content").and_then(Value::as_str).is_none()
        && opt_str(args, "content_base64").is_none()
    {
        return Err(anyhow!(
            "update_artifact requires content or content_base64"
        ));
    }
    let content = artifact_content_bytes(args)?;
    let cas = crate::control::cas::CasStore::new(cp.objects.clone());
    let object_sha = crate::control::cas::sha256_hex(&content);
    let object_key =
        crate::control::cas::artifact_object_key(&uri.namespace, &uri.artifact_id, &object_sha);
    let existing_object_metadata = cp.objects.head(&object_key).await?;
    let object_existed = existing_object_metadata.is_some();
    let revision_key = keys::artifact_revision(
        &uri.namespace,
        &uri.agent,
        &uri.session_id,
        &format!("{}-{object_sha}", uri.artifact_id),
    );
    let revision_existed = cp.kv.get(&revision_key).await?.is_some();
    let mut object_ref = match existing_object_metadata {
        Some(metadata) => crate::control::cas::object_ref_from_metadata(&object_key, &metadata),
        None => {
            cas.put_artifact(
                &uri.namespace,
                &uri.agent,
                &uri.session_id,
                &uri.artifact_id,
                &content,
                media_type,
                artifact.metadata.clone(),
            )
            .await?
        }
    };
    // The ObjectRef describes this artifact's logical representation. Reusing
    // immutable content must not rewrite its stored metadata, but the current
    // Artifact and its reference must still agree on a requested media type.
    object_ref.media_type = media_type.to_string();
    artifact.media_type = media_type.to_string();
    artifact.object_ref = Some(object_ref);
    let is_new_object = artifact
        .object_ref
        .as_ref()
        .is_some_and(|object_ref| previous_object_key.as_deref() != Some(&object_ref.key));
    if let Err(error) = record_artifact_revision(
        cp.kv.as_ref(),
        &uri.namespace,
        &uri.agent,
        &uri.session_id,
        &uri.artifact_id,
        artifact
            .object_ref
            .as_ref()
            .expect("updated artifact object ref"),
    )
    .await
    {
        if is_new_object && !object_existed {
            discard_uncommitted_artifact(
                &cas,
                cp.kv.as_ref(),
                &uri.namespace,
                &uri.agent,
                &uri.session_id,
                &uri.artifact_id,
                artifact
                    .object_ref
                    .as_ref()
                    .expect("updated artifact object ref"),
                true,
                false,
            )
            .await;
        }
        return Err(error);
    }
    let artifact_key = keys::artifact(
        &uri.namespace,
        &uri.agent,
        &uri.session_id,
        &uri.artifact_id,
    );
    if let Err(error) = cp.kv.set_msg(&artifact_key, &artifact).await {
        if is_new_object && (!object_existed || !revision_existed) {
            discard_uncommitted_artifact(
                &cas,
                cp.kv.as_ref(),
                &uri.namespace,
                &uri.agent,
                &uri.session_id,
                &uri.artifact_id,
                artifact
                    .object_ref
                    .as_ref()
                    .expect("updated artifact object ref"),
                !object_existed,
                !revision_existed,
            )
            .await;
        }
        return Err(error);
    }
    // Previous immutable revisions remain readable through durable history.
    // Session teardown reclaims every indexed revision.
    Ok(serde_json::to_string_pretty(&json!({
        "artifact": artifact_json(&artifact),
        "artifactUri": uri.encode()
    }))?)
}

pub(crate) async fn record_artifact_revision(
    kv: &dyn KeyValueStore,
    namespace: &str,
    agent: &str,
    session_id: &str,
    artifact_id: &str,
    object_ref: &data_proto::ObjectRef,
) -> Result<()> {
    let revision_id = format!("{artifact_id}-{}", object_ref.sha256);
    kv.set_msg(
        &keys::artifact_revision(namespace, agent, session_id, &revision_id),
        object_ref,
    )
    .await
}

/// Best-effort compensation after an artifact's CAS write cannot be made
/// durable. Keeping this together prevents failed writes from leaking either
/// the object or a revision index that points at it.
pub(crate) async fn discard_uncommitted_artifact(
    cas: &crate::control::cas::CasStore,
    kv: &dyn KeyValueStore,
    namespace: &str,
    agent: &str,
    session_id: &str,
    artifact_id: &str,
    object_ref: &data_proto::ObjectRef,
    remove_object: bool,
    remove_revision: bool,
) {
    if remove_revision {
        let revision_id = format!("{artifact_id}-{}", object_ref.sha256);
        if let Err(error) = kv
            .delete(&keys::artifact_revision(
                namespace,
                agent,
                session_id,
                &revision_id,
            ))
            .await
        {
            tracing::warn!(
                error = %error,
                object_key = %object_ref.key,
                "failed to remove uncommitted artifact revision"
            );
        }
    }
    if remove_object {
        if let Err(error) = cas.delete_object(&object_ref.key).await {
            tracing::warn!(
                error = %error,
                object_key = %object_ref.key,
                "failed to delete uncommitted artifact CAS object"
            );
        }
    }
}

pub async fn get_artifact_metadata(
    cp: &ControlPlane,
    _current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<String> {
    let artifact_uri = req_str(args, "artifact_uri")?;
    let (_, artifact) = resolve_artifact_uri(
        cp,
        current_agent,
        current_session,
        artifact_uri,
        OP_METADATA,
    )
    .await?;
    Ok(serde_json::to_string_pretty(&json!({
        "artifact": artifact_json(&artifact)
    }))?)
}

pub async fn grant_artifact(
    cp: &ControlPlane,
    _current_namespace: &str,
    current_agent: &str,
    current_session: &str,
    args: &Value,
) -> Result<String> {
    let artifact_uri = req_str(args, "artifact_uri")?;
    let (uri, _) =
        resolve_artifact_uri(cp, current_agent, current_session, artifact_uri, OP_READ).await?;
    let operations = args
        .get("operations")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| vec![OP_READ.to_string(), OP_METADATA.to_string()]);
    for operation in &operations {
        if !matches!(operation.as_str(), OP_READ | OP_METADATA | OP_PROMOTE) {
            return Err(anyhow!("unsupported artifact operation '{}'", operation));
        }
    }
    let ttl = args
        .get("ttl_seconds")
        .and_then(Value::as_i64)
        .map(access_expiry_from_ttl_seconds)
        .unwrap_or_else(default_access_expiry);
    let target_agent = opt_str(args, "target_agent").unwrap_or("");
    let target_session_id = opt_str(args, "target_session_id").unwrap_or("");
    let access = crate::gateway::rpc::data_proto::ArtifactAccess {
        target_agent: target_agent.to_string(),
        target_session_id: target_session_id.to_string(),
        operations,
        expires_at: ttl,
        granted_by_agent: current_agent.to_string(),
        granted_by_session_id: current_session.to_string(),
        created_at: chrono::Utc::now().timestamp_micros(),
    };
    cp.kv
        .set_msg(
            &keys::artifact_access(
                &uri.namespace,
                &uri.agent,
                &uri.session_id,
                &uri.artifact_id,
                target_agent,
                target_session_id,
            ),
            &access,
        )
        .await?;
    Ok(serde_json::to_string_pretty(&json!({
        "artifactUri": uri.encode()
    }))?)
}

pub fn artifact_content_bytes(args: &Value) -> Result<Vec<u8>> {
    if let Some(encoded) = opt_str(args, "content_base64") {
        return general_purpose::STANDARD
            .decode(encoded)
            .map_err(|err| anyhow!("content_base64 is invalid: {err}"));
    }
    Ok(args
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .as_bytes()
        .to_vec())
}

pub(crate) async fn resolve_artifact_uri(
    cp: &ControlPlane,
    current_agent: &str,
    current_session: &str,
    artifact_uri: &str,
    operation: &str,
) -> Result<(ArtifactUri, crate::gateway::rpc::data_proto::Artifact)> {
    let uri = parse_artifact_uri(artifact_uri)?;
    let artifact = cp
        .kv
        .get_msg::<crate::gateway::rpc::data_proto::Artifact>(&keys::artifact(
            &uri.namespace,
            &uri.agent,
            &uri.session_id,
            &uri.artifact_id,
        ))
        .await?
        .ok_or_else(|| anyhow!("Artifact '{}' not found", uri.artifact_id))?;
    authorize_artifact_access(
        cp,
        &uri,
        current_agent,
        current_session,
        operation,
        artifact_uri,
    )
    .await?;
    Ok((uri, artifact))
}

pub fn artifact_json(artifact: &crate::gateway::rpc::data_proto::Artifact) -> Value {
    let object_ref = artifact.object_ref.as_ref();
    json!({
        "id": artifact.id,
        "sessionId": artifact.session_id,
        "title": artifact.title,
        "mediaType": artifact.media_type,
        "createdByAgent": artifact.created_by_agent,
        "createdAt": artifact.created_at,
        "labels": artifact.labels,
        "metadata": artifact.metadata,
        "objectRef": object_ref.map(|object| json!({
            "key": object.key,
            "mediaType": object.media_type,
            "sizeBytes": object.size_bytes,
            "sha256": object.sha256,
            "filename": object.filename,
            "metadata": object.metadata,
        })).unwrap_or_else(|| json!(null))
    })
}

pub fn parse_artifact_uri(uri: &str) -> Result<ArtifactUri> {
    let rest = uri
        .trim()
        .strip_prefix("artifact://")
        .ok_or_else(|| anyhow!("artifact uri must start with 'artifact://'"))?;
    let parts = rest.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        [namespace, agent, session_id, artifact_id] => Ok(ArtifactUri {
            namespace: validate_uri_segment(namespace, "artifact namespace")?,
            agent: validate_uri_segment(agent, "artifact agent")?,
            session_id: validate_uri_segment(session_id, "artifact session")?,
            artifact_id: validate_uri_segment(artifact_id, "artifact id")?,
        }),
        _ => Err(anyhow!(
            "artifact uri must be artifact://<namespace>/<agent>/<session>/<artifact>"
        )),
    }
}

pub(crate) fn validate_uri_segment(segment: &str, name: &str) -> Result<String> {
    if segment.trim().is_empty()
        || segment.contains('/')
        || segment.contains('\0')
        || segment.chars().any(char::is_control)
    {
        return Err(anyhow!("{name} segment is invalid"));
    }
    Ok(segment.to_string())
}

pub(crate) async fn authorize_artifact_access(
    cp: &ControlPlane,
    uri: &ArtifactUri,
    current_agent: &str,
    current_session: &str,
    operation: &str,
    artifact_uri: &str,
) -> Result<()> {
    if current_agent == uri.agent && current_session == uri.session_id {
        return Ok(());
    }
    if current_agent.trim().is_empty() || current_session.trim().is_empty() {
        return Err(anyhow!(
            "artifact uri requires caller agent and session identity"
        ));
    }
    let access = cp
        .kv
        .get_msg::<crate::gateway::rpc::data_proto::ArtifactAccess>(&keys::artifact_access(
            &uri.namespace,
            &uri.agent,
            &uri.session_id,
            &uri.artifact_id,
            current_agent,
            current_session,
        ))
        .await?
        .ok_or_else(|| anyhow!("artifact access denied for '{artifact_uri}'"))?;
    if access.expires_at > 0 && access.expires_at < chrono::Utc::now().timestamp_micros() {
        return Err(anyhow!("artifact access for '{artifact_uri}' is expired"));
    }
    if !access.operations.iter().any(|op| op == operation) {
        return Err(anyhow!(
            "artifact access for '{artifact_uri}' does not allow '{operation}'"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{EmptyPubSub, MockKvStore};
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn discard_uncommitted_artifact_removes_object_and_revision() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(EmptyPubSub)).build();
        let cas = crate::control::cas::CasStore::new(cp.objects.clone());
        let object = cas
            .put_artifact(
                "ns",
                "agent",
                "session",
                "artifact",
                b"uncommitted",
                "text/plain",
                HashMap::new(),
            )
            .await
            .unwrap();
        record_artifact_revision(kv.as_ref(), "ns", "agent", "session", "artifact", &object)
            .await
            .unwrap();

        discard_uncommitted_artifact(
            &cas,
            kv.as_ref(),
            "ns",
            "agent",
            "session",
            "artifact",
            &object,
            true,
            true,
        )
        .await;

        assert!(cp.objects.head(&object.key).await.unwrap().is_none());
        assert!(kv
            .get(&keys::artifact_revision(
                "ns",
                "agent",
                "session",
                &format!("artifact-{}", object.sha256),
            ))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn discard_uncommitted_artifact_preserves_retained_revision() {
        let kv = Arc::new(MockKvStore::default());
        let cp = ControlPlane::builder(kv.clone(), Arc::new(EmptyPubSub)).build();
        let cas = crate::control::cas::CasStore::new(cp.objects.clone());
        let object = cas
            .put_artifact(
                "ns",
                "agent",
                "session",
                "artifact",
                b"retained revision",
                "text/plain",
                HashMap::new(),
            )
            .await
            .unwrap();
        let revision_key = keys::artifact_revision(
            "ns",
            "agent",
            "session",
            &format!("artifact-{}", object.sha256),
        );
        record_artifact_revision(kv.as_ref(), "ns", "agent", "session", "artifact", &object)
            .await
            .unwrap();

        discard_uncommitted_artifact(
            &cas,
            kv.as_ref(),
            "ns",
            "agent",
            "session",
            "artifact",
            &object,
            false,
            false,
        )
        .await;

        assert!(cp.objects.head(&object.key).await.unwrap().is_some());
        assert!(kv.get(&revision_key).await.unwrap().is_some());
    }
}

pub fn default_access_expiry() -> i64 {
    access_expiry_from_ttl_seconds(24 * 60 * 60)
}

pub(crate) fn access_expiry_from_ttl_seconds(ttl_seconds: i64) -> i64 {
    if ttl_seconds <= 0 {
        return default_access_expiry();
    }
    let ttl_micros = ttl_seconds.min(MAX_ACCESS_TTL_SECONDS) * 1_000_000;
    chrono::Utc::now()
        .timestamp_micros()
        .saturating_add(ttl_micros)
}
