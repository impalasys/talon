// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::gateway::rpc::resources_proto;
use std::collections::HashMap;

pub fn metadata(
    name: impl Into<String>,
    namespace: impl Into<String>,
    labels: HashMap<String, String>,
) -> resources_proto::ResourceMeta {
    resources_proto::ResourceMeta {
        name: name.into(),
        namespace: namespace.into(),
        labels,
        annotations: HashMap::new(),
        owner_references: Vec::new(),
        finalizers: Vec::new(),
        generation: 0,
        resource_version: String::new(),
        uid: String::new(),
        deletion_timestamp: None,
    }
}

pub fn common_status(phase: impl Into<String>) -> resources_proto::CommonResourceStatus {
    resources_proto::CommonResourceStatus {
        observed_generation: 0,
        phase: phase.into(),
        conditions: Vec::new(),
    }
}

pub fn agent(
    namespace: impl Into<String>,
    name: impl Into<String>,
    spec: resources_proto::AgentSpec,
    labels: HashMap<String, String>,
) -> resources_proto::Agent {
    resources_proto::Agent {
        metadata: Some(metadata(name, namespace, labels)),
        spec: Some(spec),
        status: Some(resources_proto::AgentStatus {
            observed_generation: 0,
            phase: String::new(),
            conditions: Vec::new(),
            last_session_id: None,
        }),
    }
}

pub fn agent_resource(
    namespace: impl Into<String>,
    name: impl Into<String>,
    spec: resources_proto::AgentSpec,
    labels: HashMap<String, String>,
) -> resources_proto::Resource {
    let namespace = namespace.into();
    let name = name.into();
    resources_proto::Resource {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "Agent".to_string(),
        metadata: Some(metadata(name, namespace, labels)),
        spec: Some(resources_proto::ResourceSpec {
            kind: Some(resources_proto::resource_spec::Kind::Agent(spec)),
        }),
        status: Some(resources_proto::ResourceStatus {
            kind: Some(resources_proto::resource_status::Kind::Agent(
                resources_proto::AgentStatus {
                    observed_generation: 0,
                    phase: String::new(),
                    conditions: Vec::new(),
                    last_session_id: None,
                },
            )),
        }),
    }
}

pub fn namespace(
    name: impl Into<String>,
    parent: impl Into<String>,
    labels: HashMap<String, String>,
) -> resources_proto::Namespace {
    resources_proto::Namespace {
        metadata: Some(metadata(name, String::new(), labels)),
        spec: Some(resources_proto::NamespaceSpec {
            parent: parent.into(),
        }),
        status: Some(resources_proto::NamespaceStatus {
            observed_generation: 0,
            phase: String::new(),
            conditions: Vec::new(),
            is_deleted: false,
            deleted_at: 0,
        }),
    }
}

pub fn schedule(
    namespace: impl Into<String>,
    name: impl Into<String>,
    spec: resources_proto::ScheduleSpec,
    status: resources_proto::ScheduleStatus,
    labels: HashMap<String, String>,
) -> resources_proto::Schedule {
    resources_proto::Schedule {
        metadata: Some(metadata(name, namespace, labels)),
        spec: Some(spec),
        status: Some(status),
    }
}

pub fn file_resource(
    namespace: impl Into<String>,
    name: impl Into<String>,
    spec: resources_proto::FileSpec,
    status: resources_proto::FileStatus,
    labels: HashMap<String, String>,
) -> resources_proto::Resource {
    resources_proto::Resource {
        api_version: "talon.impalasys.com/v1".to_string(),
        kind: "File".to_string(),
        metadata: Some(metadata(name, namespace, labels)),
        spec: Some(resources_proto::ResourceSpec {
            kind: Some(resources_proto::resource_spec::Kind::File(spec)),
        }),
        status: Some(resources_proto::ResourceStatus {
            kind: Some(resources_proto::resource_status::Kind::File(status)),
        }),
    }
}

pub fn workflow(
    namespace: impl Into<String>,
    name: impl Into<String>,
    spec: resources_proto::WorkflowSpec,
    labels: HashMap<String, String>,
) -> resources_proto::Workflow {
    resources_proto::Workflow {
        metadata: Some(metadata(name, namespace, labels)),
        spec: Some(spec),
        status: Some(resources_proto::WorkflowStatus {
            observed_generation: 0,
            phase: String::new(),
            conditions: Vec::new(),
        }),
    }
}

pub fn channel(
    namespace: impl Into<String>,
    name: impl Into<String>,
    spec: resources_proto::ChannelSpec,
    status: resources_proto::ChannelStatus,
    labels: HashMap<String, String>,
) -> resources_proto::Channel {
    resources_proto::Channel {
        metadata: Some(metadata(name, namespace, labels)),
        spec: Some(spec),
        status: Some(status),
    }
}

pub fn channel_subscription(
    namespace: impl Into<String>,
    name: impl Into<String>,
    spec: resources_proto::ChannelSubscriptionSpec,
    labels: HashMap<String, String>,
) -> resources_proto::ChannelSubscription {
    resources_proto::ChannelSubscription {
        metadata: Some(metadata(name, namespace, labels)),
        spec: Some(spec),
        status: Some(common_status(String::new())),
    }
}

pub trait TypedResource {
    fn metadata(&self) -> Option<&resources_proto::ResourceMeta>;
    fn metadata_mut(&mut self) -> Option<&mut resources_proto::ResourceMeta>;

    fn name(&self) -> &str {
        self.metadata()
            .map(|metadata| metadata.name.as_str())
            .unwrap_or_default()
    }

    fn namespace(&self) -> &str {
        self.metadata()
            .map(|metadata| metadata.namespace.as_str())
            .unwrap_or_default()
    }

    fn labels(&self) -> &HashMap<String, String> {
        if let Some(metadata) = self.metadata() {
            &metadata.labels
        } else {
            empty_labels()
        }
    }

    fn labels_mut(&mut self) -> Option<&mut HashMap<String, String>> {
        self.metadata_mut().map(|metadata| &mut metadata.labels)
    }

    fn set_name(&mut self, name: impl Into<String>) {
        if let Some(metadata) = self.metadata_mut() {
            metadata.name = name.into();
        }
    }

    fn set_namespace(&mut self, namespace: impl Into<String>) {
        if let Some(metadata) = self.metadata_mut() {
            metadata.namespace = namespace.into();
        }
    }
}

fn empty_labels() -> &'static HashMap<String, String> {
    static EMPTY: std::sync::OnceLock<HashMap<String, String>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(HashMap::new)
}

macro_rules! impl_typed_resource {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl TypedResource for $ty {
                fn metadata(&self) -> Option<&resources_proto::ResourceMeta> {
                    self.metadata.as_ref()
                }

                fn metadata_mut(&mut self) -> Option<&mut resources_proto::ResourceMeta> {
                    self.metadata.as_mut()
                }
            }
        )+
    };
}

impl_typed_resource!(
    resources_proto::Agent,
    resources_proto::Channel,
    resources_proto::ChannelSubscription,
    resources_proto::McpServer,
    resources_proto::Namespace,
    resources_proto::Schedule,
    resources_proto::Worker,
    resources_proto::Workflow,
);

pub trait NamespaceResourceExt {
    fn parent(&self) -> &str;
    fn is_deleted(&self) -> bool;
    fn deleted_at(&self) -> i64;
    fn set_deleted(&mut self, deleted_at: i64);
}

pub trait ChannelResourceExt {
    fn phase(&self) -> &str;
    fn created_at(&self) -> i64;
    fn updated_at(&self) -> i64;
    fn set_phase(&mut self, phase: impl Into<String>);
    fn set_created_at(&mut self, timestamp: i64);
    fn set_updated_at(&mut self, timestamp: i64);
}

impl ChannelResourceExt for resources_proto::Channel {
    fn phase(&self) -> &str {
        self.status
            .as_ref()
            .map(|status| status.phase.as_str())
            .unwrap_or_default()
    }

    fn created_at(&self) -> i64 {
        self.status
            .as_ref()
            .map(|status| status.created_at)
            .unwrap_or_default()
    }

    fn updated_at(&self) -> i64 {
        self.status
            .as_ref()
            .map(|status| status.updated_at)
            .unwrap_or_default()
    }

    fn set_phase(&mut self, phase: impl Into<String>) {
        self.status
            .get_or_insert_with(resources_proto::ChannelStatus::default)
            .phase = phase.into();
    }

    fn set_created_at(&mut self, timestamp: i64) {
        self.status
            .get_or_insert_with(resources_proto::ChannelStatus::default)
            .created_at = timestamp;
    }

    fn set_updated_at(&mut self, timestamp: i64) {
        self.status
            .get_or_insert_with(resources_proto::ChannelStatus::default)
            .updated_at = timestamp;
    }
}

pub trait ChannelSubscriptionResourceExt {
    fn channel(&self) -> &str;
    fn agent(&self) -> &str;
    fn enabled(&self) -> bool;
    fn trigger(&self) -> &str;
    fn reply_mode(&self) -> &str;
    fn context_policy(&self) -> Option<&resources_proto::ChannelContextPolicy>;
    fn subscription_metadata(&self) -> &HashMap<String, String>;
    fn spec_mut(&mut self) -> &mut resources_proto::ChannelSubscriptionSpec;
}

impl ChannelSubscriptionResourceExt for resources_proto::ChannelSubscription {
    fn channel(&self) -> &str {
        self.spec
            .as_ref()
            .map(|spec| spec.channel.as_str())
            .unwrap_or_default()
    }

    fn agent(&self) -> &str {
        self.spec
            .as_ref()
            .map(|spec| spec.agent.as_str())
            .unwrap_or_default()
    }

    fn enabled(&self) -> bool {
        self.spec.as_ref().map(|spec| spec.enabled).unwrap_or(false)
    }

    fn trigger(&self) -> &str {
        self.spec
            .as_ref()
            .map(|spec| spec.trigger.as_str())
            .unwrap_or_default()
    }

    fn reply_mode(&self) -> &str {
        self.spec
            .as_ref()
            .map(|spec| spec.reply_mode.as_str())
            .unwrap_or_default()
    }

    fn context_policy(&self) -> Option<&resources_proto::ChannelContextPolicy> {
        self.spec
            .as_ref()
            .and_then(|spec| spec.context_policy.as_ref())
    }

    fn subscription_metadata(&self) -> &HashMap<String, String> {
        if let Some(spec) = self.spec.as_ref() {
            &spec.metadata
        } else {
            empty_labels()
        }
    }

    fn spec_mut(&mut self) -> &mut resources_proto::ChannelSubscriptionSpec {
        self.spec
            .get_or_insert_with(resources_proto::ChannelSubscriptionSpec::default)
    }
}

impl NamespaceResourceExt for resources_proto::Namespace {
    fn parent(&self) -> &str {
        self.spec
            .as_ref()
            .map(|spec| spec.parent.as_str())
            .unwrap_or_default()
    }

    fn is_deleted(&self) -> bool {
        self.status
            .as_ref()
            .map(|status| status.is_deleted)
            .unwrap_or(false)
    }

    fn deleted_at(&self) -> i64 {
        self.status
            .as_ref()
            .map(|status| status.deleted_at)
            .unwrap_or_default()
    }

    fn set_deleted(&mut self, deleted_at: i64) {
        let status = self
            .status
            .get_or_insert_with(resources_proto::NamespaceStatus::default);
        status.is_deleted = true;
        status.deleted_at = deleted_at;
    }
}
