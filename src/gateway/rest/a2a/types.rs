// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SendMessageRequestJson {
    pub(super) message: A2aMessageJson,
    #[serde(default)]
    pub(super) configuration: SendMessageConfigurationJson,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SendMessageConfigurationJson {
    #[serde(default)]
    pub(super) return_immediately: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct A2aMessageJson {
    #[serde(default)]
    pub(super) message_id: String,
    pub(super) role: String,
    #[serde(default, alias = "content")]
    pub(super) parts: Vec<A2aPartJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) context_id: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct A2aPartJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) file: Option<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct A2aTaskJson {
    pub(super) id: String,
    pub(super) context_id: String,
    pub(super) status: A2aTaskStatusJson,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) artifacts: Vec<A2aArtifactJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) history: Vec<A2aMessageJson>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct A2aArtifactJson {
    pub(super) artifact_id: String,
    pub(super) name: String,
    pub(super) parts: Vec<A2aPartJson>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct A2aTaskStatusJson {
    pub(super) state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) message: Option<A2aMessageJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) timestamp: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ListTasksResponseJson {
    pub(super) tasks: Vec<A2aTaskJson>,
}

#[derive(Clone, Copy)]
pub(super) enum A2aResponseEncoding {
    Legacy,
    RestV1,
}
