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
use prost::Message;
use serde_json::json;
use std::{collections::HashMap, sync::Arc};

#[derive(Clone)]
pub struct DocumentMapper {
    cp: Arc<ControlPlane>,
}

enum MappableSource {
    SessionMessage(data_proto::SessionMessage),
    Resource(resources_proto::Resource),
}

impl MappableSource {
    fn map(
        self,
        key: &keys::ResourceKey,
        generation: u64,
        indexed_at: i64,
    ) -> Result<Vec<Document>> {
        match self {
            Self::SessionMessage(message) => Ok(map_session_message_parts(
                key, message, generation, indexed_at,
            )),
            Self::Resource(resource) => {
                map_control_plane_resource(key, resource, generation, indexed_at)
            }
        }
    }
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
        self.load_source(key)
            .await?
            .map(|source| source.map(key, generation, indexed_at))
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    async fn load_source(&self, key: &keys::ResourceKey) -> Result<Option<MappableSource>> {
        if key.kind == "Session" {
            anyhow::bail!("session index key cannot be upserted");
        }
        let Some(bytes) = self.cp.kv.get(key).await? else {
            return Ok(None);
        };
        Ok(Some(mappable_source_for_key(key, bytes.as_slice())?))
    }
}

fn mappable_source_for_key(key: &keys::ResourceKey, bytes: &[u8]) -> Result<MappableSource> {
    match key.kind.as_str() {
        "SessionMessage" => Ok(MappableSource::SessionMessage(
            data_proto::SessionMessage::decode(bytes)?,
        )),
        _ => Ok(MappableSource::Resource(
            ResourceStore::decode_stored_resource(&key.kind, bytes)?,
        )),
    }
}

fn map_control_plane_resource(
    key: &keys::ResourceKey,
    resource: resources_proto::Resource,
    generation: u64,
    indexed_at: i64,
) -> Result<Vec<Document>> {
    let current_generation = resource
        .metadata
        .as_ref()
        .map(|metadata| metadata.generation)
        .unwrap_or_default();
    if generation > 0 {
        if current_generation > generation {
            tracing::debug!(
                resource_key = key.canonical(),
                event_generation = generation,
                current_generation,
                "skipping stale resource index event"
            );
            return Ok(Vec::new());
        }
        if current_generation < generation {
            anyhow::bail!(
                "resource {} generation {} is behind index event generation {}",
                key.canonical(),
                current_generation,
                generation
            );
        }
    }
    map_control_plane_resource_documents(key, &resource, indexed_at)
}

// Document builders.
fn map_control_plane_resource_documents(
    key: &keys::ResourceKey,
    resource: &resources_proto::Resource,
    indexed_at: i64,
) -> Result<Vec<Document>> {
    let source = resource_ref(key, resource);
    let mut documents = vec![resource_metadata_document(
        key, resource, &source, indexed_at,
    )?];

    if let Some(resources_proto::resource_spec::Kind::Knowledge(spec)) =
        resource.spec.as_ref().and_then(|spec| spec.kind.as_ref())
    {
        if !spec.content.trim().is_empty() {
            documents.push(knowledge_content_document(
                key, resource, &source, spec, indexed_at,
            )?);
        }
    }

    Ok(documents)
}

fn map_session_message_parts(
    key: &keys::ResourceKey,
    message: data_proto::SessionMessage,
    generation: u64,
    indexed_at: i64,
) -> Vec<Document> {
    let agent = parent_segment(key, "Agent").unwrap_or_default();
    let session_id = parent_segment(key, "Session").unwrap_or_default();
    let role = data_proto::MessageRole::try_from(message.role)
        .ok()
        .map(|role| format!("{role:?}"))
        .unwrap_or_else(|| "ROLE_UNSPECIFIED".to_string());
    let mut docs = Vec::new();
    for part in message.parts {
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
                    (ATTR_MESSAGE_ID, message.id.clone()),
                    (ATTR_PART_ID, part.id.clone()),
                    (ATTR_PART_TYPE, "TEXT".to_string()),
                    (ATTR_ROLE, role.clone()),
                ]),
                title: format!("{agent} / {session_id}"),
                labels: message.labels.clone(),
                metadata_json: json!({ "documentKind": DOCUMENT_KIND_MESSAGE_PART }).to_string(),
                acl_scope_json: json!({
                    "namespace": key.namespace,
                    "agent": agent,
                    "session": session_id
                })
                .to_string(),
                created_at: if part.created_at == 0 {
                    message.created_at
                } else {
                    part.created_at
                },
                updated_at: if part.created_at == 0 {
                    message.created_at
                } else {
                    part.created_at
                },
                indexed_at,
                generation,
                ..document_ref(
                    document_id(&key.canonical(), DOCUMENT_KIND_MESSAGE_PART, &part.id),
                    DocumentSource {
                        namespace: key.namespace.clone(),
                        key: key.canonical(),
                        kind: KIND_SESSION_MESSAGE.to_string(),
                        name: key.name.clone(),
                        parent_kind: "Session".to_string(),
                        parent_key: keys::session(&key.namespace, &agent, &session_id).canonical(),
                        ..Default::default()
                    },
                    DOCUMENT_KIND_MESSAGE_PART.to_string(),
                    part.id.clone(),
                )
            }),
            text: part.content,
        });
    }
    docs
}

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
        resources_proto::resource_status::Kind::McpServer(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::McpServerBinding(status) => status.phase.clone(),
        resources_proto::resource_status::Kind::Knowledge(status) => status.phase.clone(),
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
    use crate::control::search::KIND_KNOWLEDGE;

    #[test]
    fn generic_resource_emits_metadata_document() {
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

        let documents = map_control_plane_resource_documents(&key, &resource, 10).unwrap();
        assert_eq!(documents.len(), 1);
        let document_ref = documents[0].r#ref.as_ref().expect("document ref");
        let source = document_ref.source.as_ref().expect("document source");
        assert_eq!(document_ref.id, format!("{}:metadata", key.canonical()));
        assert_eq!(source.kind, "Agent");
        assert_eq!(document_ref.document_kind, DOCUMENT_KIND_METADATA);
        assert!(documents[0].text.contains("support"));
        assert!(documents[0].text.contains("Ready"));
    }

    #[test]
    fn knowledge_resource_emits_metadata_and_content_documents() {
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

        let documents = map_control_plane_resource_documents(&key, &resource, 10).unwrap();
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

    #[test]
    fn raw_resource_emits_safe_metadata_only() {
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

        let documents = map_control_plane_resource_documents(&key, &resource, 10).unwrap();
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

    #[test]
    fn session_message_emits_one_text_part_document_per_text_part() {
        let key = keys::session_message("acme", "support", "s1", "m1");
        let documents = map_session_message_parts(
            &key,
            data_proto::SessionMessage {
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
            4,
            200,
        );
        assert_eq!(documents.len(), 1);
        let document_ref = documents[0].r#ref.as_ref().expect("document ref");
        assert_eq!(document_ref.id, format!("{}:part:000000", key.canonical()));
        assert_eq!(document_ref.document_kind, DOCUMENT_KIND_MESSAGE_PART);
    }
}
