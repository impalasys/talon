// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[allow(unused_imports)]
use crate::talon;

use crate::{MockKvStore, RecordingPubSub};
use chrono::Utc;
use prost::Message;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use talon::contracts::topics;
use talon::contracts::{KeyValueStore, ListOptions, ProtoKeyValueStoreExt};
use talon::identifiers::keys;
use talon::protocol::events::{MessageDirection, SessionDispatchEvent, WorkflowDispatchEvent};
use talon::protocol::{
    data_proto, events, manifests, resource_model, resource_model::TypedResource, resources_proto,
};
pub fn workflow_fixture(
    namespace: &str,
    name: &str,
    spec: resources_proto::WorkflowSpec,
) -> resources_proto::Workflow {
    talon::protocol::resource_model::workflow(namespace, name, spec, HashMap::new())
}

pub fn agent_fixture(
    namespace: &str,
    name: &str,
    spec: resources_proto::AgentSpec,
) -> resources_proto::Agent {
    resources_proto::Agent {
        metadata: Some(resources_proto::ResourceMeta {
            name: name.to_string(),
            namespace: namespace.to_string(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
            owner_references: Vec::new(),
            finalizers: Vec::new(),
            generation: 0,
            resource_version: String::new(),
            uid: String::new(),
            deletion_timestamp: None,
        }),
        spec: Some(spec),
        status: Some(resources_proto::AgentStatus::default()),
    }
}

pub fn workflow_dispatch(run: &data_proto::WorkflowRun, reason: &str) -> WorkflowDispatchEvent {
    WorkflowDispatchEvent {
        ns: run.ns.clone(),
        workflow: run.workflow.clone(),
        run_id: run.id.clone(),
        reason: reason.to_string(),
        step_id: String::new(),
        child_session_id: String::new(),
        timestamp: Utc::now().timestamp_micros(),
    }
}

pub async fn stored_run(
    kv: &MockKvStore,
    ns: &str,
    workflow: &str,
    run_id: &str,
) -> data_proto::WorkflowRun {
    kv.get_msg::<data_proto::WorkflowRun>(&keys::workflow_run(ns, workflow, run_id))
        .await
        .expect("run should load")
        .expect("run should exist")
}

pub async fn stored_step(
    kv: &MockKvStore,
    run: &data_proto::WorkflowRun,
    step_id: &str,
) -> data_proto::WorkflowStepRun {
    kv.get_msg::<data_proto::WorkflowStepRun>(&keys::workflow_step_run(
        &run.ns,
        &run.workflow,
        &run.id,
        step_id,
    ))
    .await
    .expect("step should load")
    .expect("step should exist")
}

pub async fn latest_session_dispatch(
    pubsub: &RecordingPubSub,
    agent: &str,
) -> SessionDispatchEvent {
    let published = pubsub.published.lock().await;
    published
        .iter()
        .rev()
        .filter(|(topic, _)| topic == topics::SESSION_DISPATCH_TOPIC)
        .filter_map(|(_, bytes)| SessionDispatchEvent::decode(bytes.as_slice()).ok())
        .find(|event| event.agent == agent)
        .expect("session dispatch should be published")
}

pub async fn workflow_event_types(
    kv: &MockKvStore,
    ns: &str,
    workflow: &str,
    run_id: &str,
) -> Vec<String> {
    kv.list_entries(&keys::workflow_run_event_prefix(ns, workflow, run_id), None)
        .await
        .expect("events should list")
        .into_iter()
        .map(|(_, bytes)| {
            data_proto::WorkflowRunEvent::decode(bytes.as_slice())
                .expect("event should decode")
                .r#type
        })
        .collect()
}

pub async fn event_type_count(
    kv: &MockKvStore,
    ns: &str,
    workflow: &str,
    run_id: &str,
    event_type: &str,
) -> usize {
    workflow_event_types(kv, ns, workflow, run_id)
        .await
        .into_iter()
        .filter(|current| current == event_type)
        .count()
}
