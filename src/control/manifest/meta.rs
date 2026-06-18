// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::control::resource_model::{self, ChannelResourceExt, TypedResource};
use crate::gateway::rpc::{
    manifests,
    protobuf_value::{value, ListValue, Value},
    resources_proto,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawManifest {
    pub api_version: String,
    pub kind: String,
    pub metadata: serde_yaml::Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceYamlDocument {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    #[serde(default)]
    spec: serde_yaml::Value,
    #[serde(default, skip_serializing_if = "is_empty_yaml_value")]
    status: serde_yaml::Value,
}

fn is_empty_yaml_value(value: &serde_yaml::Value) -> bool {
    match value {
        serde_yaml::Value::Null => true,
        serde_yaml::Value::Mapping(mapping) => mapping.is_empty(),
        _ => false,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesiredResourceManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    #[serde(default)]
    spec: serde_yaml::Value,
    status: Option<serde_yaml::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObjectMetaManifest {
    name: String,
    #[serde(default)]
    namespace: String,
    #[serde(default)]
    labels: HashMap<String, String>,
    #[serde(default)]
    annotations: HashMap<String, String>,
}
