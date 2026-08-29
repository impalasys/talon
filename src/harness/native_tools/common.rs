// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only
use crate::gateway::rpc::{manifests, protobuf_value::value::Kind as ProtoValueKind};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;

pub(crate) fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| anyhow!("'{}' is required", key))
}

pub(crate) fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

pub(crate) fn opt_usize(args: &Value, key: &str) -> Option<usize> {
    args.get(key).and_then(Value::as_u64).map(|v| v as usize)
}

pub(crate) fn opt_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

pub(crate) fn string_map(value: Option<&Value>) -> HashMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn string_vec(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::trim))
                .filter(|v| !v.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn has_capability_action(
    spec: &manifests::AgentSpec,
    capability: &str,
    action: &str,
) -> bool {
    spec.capabilities
        .get(capability)
        .map(|a| {
            a.values.iter().any(
                |v| matches!(v.kind.as_ref(), Some(ProtoValueKind::StringValue(c)) if c == action),
            )
        })
        .unwrap_or(false)
}

pub(crate) fn require_capability(
    spec: &manifests::AgentSpec,
    capability: &str,
    action: &str,
) -> Result<()> {
    if has_capability_action(spec, capability, action) {
        Ok(())
    } else {
        Err(anyhow!(
            "agent does not have capability '{}:{}'",
            capability,
            action
        ))
    }
}

pub(crate) fn require_file_read(spec: &manifests::AgentSpec) -> Result<()> {
    if has_capability_action(spec, "files", "read")
        || has_capability_action(spec, "files", "inspect")
    {
        Ok(())
    } else {
        Err(anyhow!("agent does not have capability 'files:read'"))
    }
}

pub(crate) fn normalize_logical_path(path: &str) -> Result<String> {
    let p = path.trim();
    if p.is_empty() {
        return Err(anyhow!("path is required"));
    }
    if !p.starts_with('/') {
        return Err(anyhow!("path must be absolute"));
    }
    if p.contains("//") || p.contains('\0') || p.contains("..") {
        return Err(anyhow!("path is not normalized"));
    }
    Ok(p.trim_end_matches('/').to_string())
}
