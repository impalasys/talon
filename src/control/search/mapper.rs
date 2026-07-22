// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{
    document_id, document_ref, Document, DocumentSource, ATTR_AGENT, ATTR_CHANNEL, ATTR_MESSAGE_ID,
    ATTR_PART_ID, ATTR_PART_TYPE, ATTR_ROLE, ATTR_SESSION_ID, DOCUMENT_KIND_CONTENT,
    DOCUMENT_KIND_MESSAGE_PART, DOCUMENT_KIND_METADATA, KIND_SESSION_MESSAGE,
};
use crate::control::resources::ResourceStore;
use crate::control::{keys, ControlPlane};
use crate::gateway::rpc::{data_proto, resources_proto};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use prost::Message;
use serde_json::json;
use std::{collections::HashMap, sync::Arc};

const MAX_INDEXED_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct DocumentMapper {
    cp: Arc<ControlPlane>,
}

#[async_trait]
trait MappableSource: Send + Sync {
    async fn map(self: Box<Self>, generation: u64, indexed_at: i64) -> Result<Vec<Document>>;
}

struct SessionMessageSource {
    key: keys::ResourceKey,
    message: data_proto::SessionMessage,
}

struct ControlPlaneResourceSource {
    cp: Arc<ControlPlane>,
    key: keys::ResourceKey,
    resource: resources_proto::Resource,
}

// Public entrypoint and source dispatch.
impl DocumentMapper {
    pub fn new(cp: Arc<ControlPlane>) -> Self {
        Self { cp }
    }

    pub async fn map_key(
        &self,
        key: &keys::ResourceKey,
        generation: u64,
        indexed_at: i64,
    ) -> Result<Vec<Document>> {
        let Some(source) = self.load_source(key).await? else {
            return Ok(Vec::new());
        };
        source.map(generation, indexed_at).await
    }

    async fn load_source(
        &self,
        key: &keys::ResourceKey,
    ) -> Result<Option<Box<dyn MappableSource>>> {
        if key.kind == "Session" {
            anyhow::bail!("session index key cannot be upserted");
        }
        let Some(bytes) = self.cp.kv.get(key).await? else {
            return Ok(None);
        };
        Ok(Some(self.mappable_source_for_key(key, bytes.as_slice())?))
    }

    fn mappable_source_for_key(
        &self,
        key: &keys::ResourceKey,
        bytes: &[u8],
    ) -> Result<Box<dyn MappableSource>> {
        match key.kind.as_str() {
            "SessionMessage" => Ok(Box::new(SessionMessageSource {
                key: key.clone(),
                message: data_proto::SessionMessage::decode(bytes)?,
            })),
            _ => Ok(Box::new(ControlPlaneResourceSource {
                cp: self.cp.clone(),
                key: key.clone(),
                resource: ResourceStore::decode_stored_resource(&key.kind, bytes)?,
            })),
        }
    }
}

#[async_trait]
impl MappableSource for SessionMessageSource {
    async fn map(self: Box<Self>, generation: u64, indexed_at: i64) -> Result<Vec<Document>> {
        let agent = parent_segment(&self.key, "Agent").unwrap_or_default();
        let session_id = parent_segment(&self.key, "Session").unwrap_or_default();
        let role = data_proto::MessageRole::try_from(self.message.role)
            .ok()
            .map(|role| format!("{role:?}"))
            .unwrap_or_else(|| "ROLE_UNSPECIFIED".to_string());
        let mut docs = Vec::new();
        for part in self.message.parts {
            if part.part_type != data_proto::SessionMessagePartType::Text as i32 {
                continue;
            }
            if part.content.trim().is_empty() {
                continue;
            }
            docs.push(Document {
                r#ref: Some(data_proto::DocumentRef {
                    attributes: attributes([
                        (ATTR_AGENT, agent.clone()),
                        (ATTR_SESSION_ID, session_id.clone()),
                        (ATTR_MESSAGE_ID, self.message.id.clone()),
                        (ATTR_PART_ID, part.id.clone()),
                        (ATTR_PART_TYPE, "TEXT".to_string()),
                        (ATTR_ROLE, role.clone()),
                    ]),
                    title: format!("{agent} / {session_id}"),
                    labels: self.message.labels.clone(),
                    metadata_json: json!({ "documentKind": DOCUMENT_KIND_MESSAGE_PART })
                        .to_string(),
                    acl_scope_json: json!({
                        "namespace": self.key.namespace,
                        "agent": agent,
                        "session": session_id
                    })
                    .to_string(),
                    created_at: if part.created_at == 0 {
                        self.message.created_at
                    } else {
                        part.created_at
                    },
                    updated_at: if part.created_at == 0 {
                        self.message.created_at
                    } else {
                        part.created_at
                    },
                    indexed_at,
                    generation,
                    ..document_ref(
                        document_id(&self.key.canonical(), DOCUMENT_KIND_MESSAGE_PART, &part.id),
                        DocumentSource {
                            namespace: self.key.namespace.clone(),
                            key: self.key.canonical(),
                            kind: KIND_SESSION_MESSAGE.to_string(),
                            name: self.key.name.clone(),
                            parent_kind: "Session".to_string(),
                            parent_key: keys::session(&self.key.namespace, &agent, &session_id)
                                .canonical(),
                            ..Default::default()
                        },
                        DOCUMENT_KIND_MESSAGE_PART.to_string(),
                        part.id.clone(),
                    )
                }),
                text: part.content,
            });
        }
        Ok(docs)
    }
}

#[async_trait]
impl MappableSource for ControlPlaneResourceSource {
    async fn map(self: Box<Self>, generation: u64, indexed_at: i64) -> Result<Vec<Document>> {
        let current_generation = self
            .resource
            .metadata
            .as_ref()
            .map(|metadata| metadata.generation)
            .unwrap_or_default();
        if generation > 0 {
            if current_generation > generation {
                tracing::debug!(
                    resource_key = self.key.canonical(),
                    event_generation = generation,
                    current_generation,
                    "skipping stale resource index event"
                );
                return Ok(Vec::new());
            }
            if current_generation < generation {
                anyhow::bail!(
                    "resource {} generation {} is behind index event generation {}",
                    self.key.canonical(),
                    current_generation,
                    generation
                );
            }
        }
        let source = resource_ref(&self.key, &self.resource);
        let mut documents = vec![resource_metadata_document(
            &self.key,
            &self.resource,
            &source,
            indexed_at,
        )?];

        if let Some(resources_proto::resource_spec::Kind::Knowledge(spec)) = self
            .resource
            .spec
            .as_ref()
            .and_then(|spec| spec.kind.as_ref())
        {
            if !spec.content.trim().is_empty() {
                documents.push(knowledge_content_document(
                    &self.key,
                    &self.resource,
                    &source,
                    spec,
                    indexed_at,
                )?);
            }
        }

        if let Some(resources_proto::resource_spec::Kind::File(spec)) = self
            .resource
            .spec
            .as_ref()
            .and_then(|spec| spec.kind.as_ref())
        {
            if should_index_file(spec) {
                if let Some(document) = file_content_document(
                    self.cp.as_ref(),
                    &self.key,
                    &self.resource,
                    &source,
                    spec,
                    indexed_at,
                )
                .await?
                {
                    documents.push(document);
                }
            }
        }

        Ok(documents)
    }
}

// Document builders.
fn resource_metadata_document(
    key: &keys::ResourceKey,
    resource: &resources_proto::Resource,
    source: &DocumentSource,
    indexed_at: i64,
) -> Result<Document> {
    let meta = resource
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow!("resource metadata is required"))?;
    let (created_at, updated_at) = resource_timestamps(resource);
    let text = metadata_text(resource);
    Ok(Document {
        r#ref: Some(data_proto::DocumentRef {
            attributes: metadata_attributes(key, resource),
            title: format!("{}/{}", source.kind, meta.name),
            labels: meta.labels.clone(),
            metadata_json: json!({
                "documentKind": DOCUMENT_KIND_METADATA,
                "apiVersion": resource.api_version,
                "kind": resource.kind,
                "name": meta.name,
                "namespace": meta.namespace,
                "uid": meta.uid,
                "generation": meta.generation,
                "resourceVersion": meta.resource_version,
                "annotations": meta.annotations,
                "ownerReferences": meta.owner_references,
                "phase": status_phase(resource),
            })
            .to_string(),
            acl_scope_json: acl_scope_json(key, resource),
            created_at,
            updated_at,
            indexed_at,
            generation: meta.generation,
            ..document_ref(
                document_id(&source.key, DOCUMENT_KIND_METADATA, ""),
                source.clone(),
                DOCUMENT_KIND_METADATA.to_string(),
                String::new(),
            )
        }),
        text,
    })
}

fn knowledge_content_document(
    key: &keys::ResourceKey,
    resource: &resources_proto::Resource,
    source: &DocumentSource,
    spec: &resources_proto::KnowledgeSpec,
    indexed_at: i64,
) -> Result<Document> {
    let meta = resource
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow!("resource metadata is required"))?;
    Ok(Document {
        r#ref: Some(data_proto::DocumentRef {
            title: spec.path.clone(),
            labels: meta.labels.clone(),
            metadata_json: json!({
                "documentKind": DOCUMENT_KIND_CONTENT,
                "path": spec.path,
                "name": meta.name,
                "uid": meta.uid,
                "resourceVersion": meta.resource_version,
            })
            .to_string(),
            acl_scope_json: acl_scope_json(key, resource),
            indexed_at,
            generation: meta.generation,
            ..document_ref(
                document_id(&source.key, DOCUMENT_KIND_CONTENT, ""),
                source.clone(),
                DOCUMENT_KIND_CONTENT.to_string(),
                String::new(),
            )
        }),
        text: spec.content.clone(),
    })
}

fn should_index_file(spec: &resources_proto::FileSpec) -> bool {
    matches!(
        resources_proto::FileIndexPolicy::try_from(spec.index_policy).ok(),
        Some(resources_proto::FileIndexPolicy::Search)
            | Some(resources_proto::FileIndexPolicy::Retrieval)
    )
}

fn is_text_media_type(media_type: &str) -> bool {
    let media_type = media_type
        .split_once(';')
        .map(|(value, _)| value)
        .unwrap_or(media_type)
        .trim()
        .to_ascii_lowercase();
    media_type.starts_with("text/")
        || media_type.ends_with("+json")
        || media_type.ends_with("+xml")
        || matches!(
            media_type.as_str(),
            "application/json"
                | "application/javascript"
                | "application/typescript"
                | "application/xml"
                | "application/toml"
                | "application/yaml"
                | "application/x-yaml"
                | "application/markdown"
                | "application/x-httpd-php"
                | "application/x-python"
                | "application/x-ruby"
                | "application/x-sh"
                | "application/x-shellscript"
        )
}

fn file_purpose_name(value: i32) -> &'static str {
    match resources_proto::FilePurpose::try_from(value).ok() {
        Some(resources_proto::FilePurpose::Memory) => "MEMORY",
        Some(resources_proto::FilePurpose::Artifact) => "ARTIFACT",
        _ => "UNSPECIFIED",
    }
}

fn file_index_policy_name(value: i32) -> &'static str {
    match resources_proto::FileIndexPolicy::try_from(value).ok() {
        Some(resources_proto::FileIndexPolicy::None) => "NONE",
        Some(resources_proto::FileIndexPolicy::Search) => "SEARCH",
        Some(resources_proto::FileIndexPolicy::Retrieval) => "RETRIEVAL",
        _ => "UNSPECIFIED",
    }
}

async fn file_content_document(
    cp: &ControlPlane,
    key: &keys::ResourceKey,
    resource: &resources_proto::Resource,
    source: &DocumentSource,
    spec: &resources_proto::FileSpec,
    indexed_at: i64,
) -> Result<Option<Document>> {
    if !is_text_media_type(&spec.media_type) {
        return Ok(None);
    }
    let meta = resource
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow!("resource metadata is required"))?;
    let Some(object_ref) = resource
        .status
        .as_ref()
        .and_then(|status| status.kind.as_ref())
        .and_then(|kind| match kind {
            resources_proto::resource_status::Kind::File(status) => status.object_ref.as_ref(),
            _ => None,
        })
    else {
        return Ok(None);
    };
    if object_ref.size_bytes > MAX_INDEXED_FILE_BYTES {
        tracing::warn!(
            resource = %key.canonical(),
            object_key = %object_ref.key,
            file = %meta.name,
            size_bytes = object_ref.size_bytes,
            max_bytes = MAX_INDEXED_FILE_BYTES,
            "file object is too large for content indexing; skipping content document"
        );
        return Ok(None);
    }
    let cas = crate::control::cas::CasStore::new(cp.objects.clone());
    let Some(object) = (match cas.get_object_decoded(&object_ref.key).await {
        Ok(object) => object,
        Err(error) => {
            tracing::warn!(
                error = %error,
                resource = %key.canonical(),
                object_key = %object_ref.key,
                "failed to fetch file object for indexing; skipping content document"
            );
            return Ok(None);
        }
    }) else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&object.bytes).into_owned();
    if text.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(Document {
        r#ref: Some(data_proto::DocumentRef {
            title: spec.path.clone(),
            labels: meta.labels.clone(),
            metadata_json: json!({
                "documentKind": DOCUMENT_KIND_CONTENT,
                "path": spec.path,
                "name": meta.name,
                "uid": meta.uid,
                "resourceVersion": meta.resource_version,
                "purpose": file_purpose_name(spec.purpose),
                "indexPolicy": file_index_policy_name(spec.index_policy),
                "objectKey": object_ref.key,
                "sha256": object_ref.sha256,
            })
            .to_string(),
            acl_scope_json: acl_scope_json(key, resource),
            indexed_at,
            generation: meta.generation,
            ..document_ref(
                document_id(&source.key, DOCUMENT_KIND_CONTENT, ""),
                source.clone(),
                DOCUMENT_KIND_CONTENT.to_string(),
                String::new(),
            )
        }),
        text,
    }))
}

// Common helpers.
fn resource_ref(key: &keys::ResourceKey, resource: &resources_proto::Resource) -> DocumentSource {
    let meta = resource.metadata.as_ref();
    let (parent_kind, parent_key) = parent_ref(key);
    DocumentSource {
        namespace: key.namespace.clone(),
        kind: resource.kind.clone(),
        key: key.canonical(),
        name: meta
            .map(|meta| meta.name.clone())
            .unwrap_or_else(|| key.name.clone()),
        parent_kind,
        parent_key,
        uid: meta.map(|meta| meta.uid.clone()).unwrap_or_default(),
        generation: meta.map(|meta| meta.generation).unwrap_or_default(),
        resource_version: meta
            .map(|meta| meta.resource_version.clone())
            .unwrap_or_default(),
    }
}

fn metadata_attributes(
    key: &keys::ResourceKey,
    resource: &resources_proto::Resource,
) -> HashMap<String, String> {
    attributes([
        (ATTR_AGENT, derived_agent(key, resource)),
        (ATTR_SESSION_ID, derived_session(key, resource)),
        (ATTR_CHANNEL, derived_channel(key, resource)),
    ])
}

fn attributes(values: impl IntoIterator<Item = (&'static str, String)>) -> HashMap<String, String> {
    values
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn parent_ref(key: &keys::ResourceKey) -> (String, String) {
    let Ok(segments) = key.parent_segments() else {
        return ("Namespace".to_string(), key.namespace.clone());
    };
    let Some(parent) = segments.last() else {
        return ("Namespace".to_string(), key.namespace.clone());
    };
    (parent.kind.clone(), key.parent_path.clone())
}

fn metadata_text(resource: &resources_proto::Resource) -> String {
    let Some(meta) = resource.metadata.as_ref() else {
        return resource.kind.clone();
    };
    let mut fields = vec![
        resource.kind.clone(),
        meta.name.clone(),
        meta.namespace.clone(),
        status_phase(resource),
    ];
    fields.extend(
        meta.labels
            .iter()
            .flat_map(|(key, value)| [key.clone(), value.clone()]),
    );
    fields.extend(
        meta.annotations
            .iter()
            .flat_map(|(key, value)| [key.clone(), value.clone()]),
    );
    for owner in &meta.owner_references {
        fields.push(owner.kind.clone());
        fields.push(owner.namespace.clone());
        fields.push(owner.name.clone());
    }
    fields.extend(safe_spec_text(resource));
    fields.retain(|value| !value.trim().is_empty());
    fields.join(" ")
}

fn safe_spec_text(resource: &resources_proto::Resource) -> Vec<String> {
    match resource.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
        Some(resources_proto::resource_spec::Kind::Knowledge(spec)) => {
            vec![spec.path.clone()]
        }
        Some(resources_proto::resource_spec::Kind::Namespace(spec)) => {
            vec![spec.parent.clone()]
        }
        Some(resources_proto::resource_spec::Kind::Channel(spec)) => {
            vec![spec.title.clone()]
        }
        Some(resources_proto::resource_spec::Kind::ChannelSubscription(spec)) => vec![
            spec.channel.clone(),
            spec.agent.clone(),
            spec.trigger.clone(),
            spec.reply_mode.clone(),
        ],
        Some(resources_proto::resource_spec::Kind::Session(spec)) => {
            vec![spec.agent.clone()]
        }
        Some(resources_proto::resource_spec::Kind::Schedule(spec)) => {
            let mut fields = vec![
                spec.kind.clone(),
                spec.cron.clone(),
                spec.run_at.clone(),
                spec.timezone.clone(),
            ];
            if let Some(target) = spec.target.as_ref() {
                fields.push(target.agent.clone());
                fields.push(target.session_id.clone());
                fields.push(target.workflow.clone());
            }
            fields
        }
        Some(resources_proto::resource_spec::Kind::Workflow(spec)) => {
            vec![spec.description.clone()]
        }
        Some(resources_proto::resource_spec::Kind::Skill(spec)) => {
            vec![spec.description.clone()]
        }
        _ => Vec::new(),
    }
}

fn status_phase(resource: &resources_proto::Resource) -> String {
    let Some(status) = resource
        .status
        .as_ref()
        .and_then(|status| status.kind.as_ref())
    else {
        return String::new();
    };
    match status {
        resources_proto::resource_status::Kind::Agent(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Workflow(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Schedule(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Channel(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::ChannelSubscription(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::ConnectorClass(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Connector(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::McpServer(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Knowledge(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Secret(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::File(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Task(status) => {
            resources_proto::TaskPhase::try_from(status.phase)
                .map(|phase| {
                    phase
                        .as_str_name()
                        .trim_start_matches("TASK_PHASE_")
                        .to_string()
                })
                .unwrap_or_default()
        }
        resources_proto::resource_status::Kind::Namespace(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Session(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Skill(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Template(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Deployment(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::DeploymentReplica(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::SandboxClass(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::SandboxPolicy(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Sandbox(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Worker(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::UsagePolicy(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Raw(_) => String::new(),
    }
}

fn resource_timestamps(resource: &resources_proto::Resource) -> (i64, i64) {
    match resource
        .status
        .as_ref()
        .and_then(|status| status.kind.as_ref())
    {
        Some(resources_proto::resource_status::Kind::Channel(status)) => {
            (status.created_at, status.updated_at)
        }
        Some(resources_proto::resource_status::Kind::Session(status)) => {
            (status.created_at, status.last_active)
        }
        _ => (0, 0),
    }
}

fn acl_scope_json(key: &keys::ResourceKey, resource: &resources_proto::Resource) -> String {
    json!({
        "namespace": key.namespace,
        "agent": derived_agent(key, resource),
        "session": derived_session(key, resource),
        "channel": derived_channel(key, resource),
    })
    .to_string()
}

fn derived_agent(key: &keys::ResourceKey, resource: &resources_proto::Resource) -> String {
    if let Some(agent) = parent_segment(key, "Agent") {
        return agent;
    }
    match resource.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
        Some(resources_proto::resource_spec::Kind::Session(spec)) => spec.agent.clone(),
        Some(resources_proto::resource_spec::Kind::ChannelSubscription(spec)) => spec.agent.clone(),
        Some(resources_proto::resource_spec::Kind::Schedule(spec)) => spec
            .target
            .as_ref()
            .map(|target| target.agent.clone())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn derived_session(key: &keys::ResourceKey, resource: &resources_proto::Resource) -> String {
    if let Some(session) = parent_segment(key, "Session") {
        return session;
    }
    if resource.kind == "Session" {
        return resource
            .metadata
            .as_ref()
            .map(|meta| meta.name.clone())
            .unwrap_or_default();
    }
    match resource.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
        Some(resources_proto::resource_spec::Kind::Schedule(spec)) => spec
            .target
            .as_ref()
            .map(|target| target.session_id.clone())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn derived_channel(key: &keys::ResourceKey, resource: &resources_proto::Resource) -> String {
    if let Some(channel) = parent_segment(key, "Channel") {
        return channel;
    }
    if resource.kind == "Channel" {
        return resource
            .metadata
            .as_ref()
            .map(|meta| meta.name.clone())
            .unwrap_or_default();
    }
    match resource.spec.as_ref().and_then(|spec| spec.kind.as_ref()) {
        Some(resources_proto::resource_spec::Kind::ChannelSubscription(spec)) => {
            spec.channel.clone()
        }
        _ => String::new(),
    }
}

fn parent_segment(key: &keys::ResourceKey, kind: &str) -> Option<String> {
    key.parent_segments()
        .ok()?
        .into_iter()
        .find(|segment| segment.kind == kind)
        .map(|segment| segment.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::object_store::ObjectMetadata;
    use crate::control::search::KIND_KNOWLEDGE;

    #[tokio::test]
    async fn generic_resource_emits_metadata_document() {
        let key = keys::ResourceKey::new("acme", &[], "Agent", "support");
        let resource = resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Agent".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "support".to_string(),
                namespace: "acme".to_string(),
                labels: [("team".to_string(), "care".to_string())]
                    .into_iter()
                    .collect(),
                generation: 2,
                resource_version: "rv1".to_string(),
                uid: "uid1".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Agent(
                    resources_proto::AgentSpec::default(),
                )),
            }),
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resources_proto::resource_status::Kind::Agent(
                    resources_proto::AgentStatus {
                        phase: "Ready".to_string(),
                        ..Default::default()
                    },
                )),
            }),
        };

        let documents = Box::new(ControlPlaneResourceSource {
            cp: Arc::new(ControlPlane::noop()),
            key: key.clone(),
            resource,
        })
        .map(0, 10)
        .await
        .unwrap();
        assert_eq!(documents.len(), 1);
        let document_ref = documents[0].r#ref.as_ref().expect("document ref");
        let source = document_ref.source.as_ref().expect("document source");
        assert_eq!(document_ref.id, format!("{}:metadata", key.canonical()));
        assert_eq!(source.kind, "Agent");
        assert_eq!(document_ref.document_kind, DOCUMENT_KIND_METADATA);
        assert!(documents[0].text.contains("support"));
        assert!(documents[0].text.contains("Ready"));
    }

    #[tokio::test]
    async fn knowledge_resource_emits_metadata_and_content_documents() {
        let key = keys::ResourceKey::new("acme", &[], KIND_KNOWLEDGE, "refunds");
        let resource = resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: KIND_KNOWLEDGE.to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "refunds".to_string(),
                namespace: "acme".to_string(),
                generation: 3,
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Knowledge(
                    resources_proto::KnowledgeSpec {
                        path: "policies/refunds.md".to_string(),
                        content: "Refund policy details".to_string(),
                    },
                )),
            }),
            ..Default::default()
        };

        let documents = Box::new(ControlPlaneResourceSource {
            cp: Arc::new(ControlPlane::noop()),
            key: key.clone(),
            resource,
        })
        .map(0, 10)
        .await
        .unwrap();
        assert_eq!(documents.len(), 2);
        assert_eq!(
            documents[0]
                .r#ref
                .as_ref()
                .expect("document ref")
                .document_kind,
            DOCUMENT_KIND_METADATA
        );
        assert_eq!(
            documents[1]
                .r#ref
                .as_ref()
                .expect("document ref")
                .document_kind,
            DOCUMENT_KIND_CONTENT
        );
        assert_eq!(documents[1].text, "Refund policy details");
    }

    #[tokio::test]
    async fn file_resource_with_retrieval_policy_indexes_object_content() {
        let cp = Arc::new(ControlPlane::noop());
        cp.objects
            .put(
                "cas/acme/files/file-uid/sha",
                b"Memory file content",
                ObjectMetadata {
                    media_type: "text/markdown".to_string(),
                    filename: "guide.md".to_string(),
                    sha256: "sha".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let key = keys::ResourceKey::new("acme", &[], "File", "guide-md");
        let resource = resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "File".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "guide-md".to_string(),
                namespace: "acme".to_string(),
                generation: 4,
                uid: "file-uid".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::File(
                    resources_proto::FileSpec {
                        path: "/memory/guide.md".to_string(),
                        media_type: "text/markdown".to_string(),
                        purpose: resources_proto::FilePurpose::Memory as i32,
                        index_policy: resources_proto::FileIndexPolicy::Retrieval as i32,
                        retention: resources_proto::FileRetention::Retained as i32,
                    },
                )),
            }),
            status: Some(resources_proto::ResourceStatus {
                kind: Some(resources_proto::resource_status::Kind::File(
                    resources_proto::FileStatus {
                        object_ref: Some(resources_proto::FileObjectRef {
                            key: "cas/acme/files/file-uid/sha".to_string(),
                            media_type: "text/markdown".to_string(),
                            size_bytes: 19,
                            sha256: "sha".to_string(),
                            filename: "guide.md".to_string(),
                            metadata: HashMap::new(),
                        }),
                        ..Default::default()
                    },
                )),
            }),
        };

        let documents = Box::new(ControlPlaneResourceSource { cp, key, resource })
            .map(0, 10)
            .await
            .unwrap();

        assert_eq!(documents.len(), 2);
        assert_eq!(documents[1].text, "Memory file content");
        assert_eq!(
            documents[1]
                .r#ref
                .as_ref()
                .expect("document ref")
                .document_kind,
            DOCUMENT_KIND_CONTENT
        );
    }

    #[tokio::test]
    async fn raw_resource_emits_safe_metadata_only() {
        let key = keys::ResourceKey::new("acme", &[], "Custom", "raw-one");
        let resource = resources_proto::Resource {
            api_version: "talon.impalasys.com/v1".to_string(),
            kind: "Custom".to_string(),
            metadata: Some(resources_proto::ResourceMeta {
                name: "raw-one".to_string(),
                namespace: "acme".to_string(),
                labels: [("visible".to_string(), "yes".to_string())]
                    .into_iter()
                    .collect(),
                ..Default::default()
            }),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::Raw(
                    resources_proto::RawResourceSpec {
                        json: r#"{"secret":"do-not-index"}"#.to_string(),
                    },
                )),
            }),
            ..Default::default()
        };

        let documents = Box::new(ControlPlaneResourceSource {
            cp: Arc::new(ControlPlane::noop()),
            key: key.clone(),
            resource,
        })
        .map(0, 10)
        .await
        .unwrap();
        assert_eq!(documents.len(), 1);
        assert_eq!(
            documents[0]
                .r#ref
                .as_ref()
                .expect("document ref")
                .document_kind,
            DOCUMENT_KIND_METADATA
        );
        assert!(documents[0].text.contains("raw-one"));
        assert!(!documents[0].text.contains("do-not-index"));
    }

    #[tokio::test]
    async fn session_message_emits_one_text_part_document_per_text_part() {
        let key = keys::session_message("acme", "support", "s1", "m1");
        let documents = Box::new(SessionMessageSource {
            key: key.clone(),
            message: data_proto::SessionMessage {
                id: "m1".to_string(),
                role: data_proto::MessageRole::RoleUser as i32,
                created_at: 100,
                parts: vec![
                    data_proto::SessionMessagePart {
                        id: "000000".to_string(),
                        part_type: data_proto::SessionMessagePartType::Text as i32,
                        content: "hello".to_string(),
                        created_at: 101,
                        ..Default::default()
                    },
                    data_proto::SessionMessagePart {
                        id: "000001".to_string(),
                        part_type: data_proto::SessionMessagePartType::ToolCall as i32,
                        content: "ignored".to_string(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        })
        .map(4, 200)
        .await
        .unwrap();
        assert_eq!(documents.len(), 1);
        let document_ref = documents[0].r#ref.as_ref().expect("document ref");
        assert_eq!(document_ref.id, format!("{}:part:000000", key.canonical()));
        assert_eq!(document_ref.document_kind, DOCUMENT_KIND_MESSAGE_PART);
    }
}
