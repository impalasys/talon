// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequestJson {
    pub message: A2aMessageJson,
    #[serde(default)]
    pub configuration: SendMessageConfigurationJson,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageConfigurationJson {
    #[serde(default)]
    pub return_immediately: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2aMessageJson {
    #[serde(default)]
    pub message_id: String,
    pub role: String,
    #[serde(default, alias = "content")]
    pub parts: Vec<A2aPartJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2aPartJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2aTaskJson {
    pub id: String,
    pub context_id: String,
    pub status: A2aTaskStatusJson,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<A2aArtifactJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<A2aMessageJson>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2aArtifactJson {
    pub artifact_id: String,
    pub name: String,
    pub parts: Vec<A2aPartJson>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2aTaskStatusJson {
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<A2aMessageJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksResponseJson {
    pub tasks: Vec<A2aTaskJson>,
}

#[derive(Clone, Copy)]
pub enum A2aResponseEncoding {
    Legacy,
    RestV1,
}
