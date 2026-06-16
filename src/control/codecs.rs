// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use serde_json::Value;

use crate::gateway::rpc::resources_proto;

#[derive(Default, Clone)]
pub struct ResourceCodecRegistry;

impl ResourceCodecRegistry {
    pub fn new() -> Self {
        Self
    }

    pub fn decode_flat(
        &self,
        kind: &str,
        spec: Value,
        status: Value,
    ) -> Result<(
        resources_proto::ResourceSpec,
        resources_proto::ResourceStatus,
    )> {
        crate::control::manifest::resource_spec_status_from_json(
            kind,
            &serde_json::to_string(&spec)?,
            &serde_json::to_string(&status)?,
        )
    }

    pub fn canonical_spec_json(&self, resource: &resources_proto::Resource) -> Result<Value> {
        let rendered = crate::control::manifest::render_resource_yaml(resource)?;
        let yaml: serde_yaml::Value = serde_yaml::from_str(&rendered)?;
        let json = serde_json::to_value(yaml)?;
        Ok(json.get("spec").cloned().unwrap_or(Value::Null))
    }

    pub fn canonical_status_json(&self, resource: &resources_proto::Resource) -> Result<Value> {
        let rendered = crate::control::manifest::render_resource_yaml(resource)?;
        let yaml: serde_yaml::Value = serde_yaml::from_str(&rendered)?;
        let json = serde_json::to_value(yaml)?;
        Ok(json.get("status").cloned().unwrap_or(Value::Null))
    }
}
