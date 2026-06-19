// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use serde_json::Value;

use super::ExecSpec;

pub fn shell_command(spec: &ExecSpec) -> String {
    let mut command = shell_escape(&spec.command);
    for arg in &spec.args {
        command.push(' ');
        command.push_str(&shell_escape(arg));
    }
    if spec.cwd.trim().is_empty() {
        command
    } else {
        format!("cd {} && {}", shell_escape(&spec.cwd), command)
    }
}

pub fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn provider_for_prefix(provider: &str) -> &str {
    match provider {
        "" | "mock" => "mock",
        "local-docker" => "docker",
        other => other,
    }
}

pub fn split_backend_id(backend_id: &str) -> (&str, &str) {
    match backend_id.split_once(':') {
        Some((provider @ ("mock" | "docker" | "daytona" | "e2b"), id)) => (provider, id),
        _ => ("mock", backend_id),
    }
}

pub(super) fn docker_parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_string(),
        Some(index) => path[..index].to_string(),
        None => ".".to_string(),
    }
}

pub(super) fn json_string_at(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn json_string_array_at(value: &Value, pointer: &str) -> Vec<String> {
    value
        .pointer(pointer)
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .map(|items: Vec<String>| {
            items
                .into_iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn json_bool_at(value: &Value, pointer: &str) -> Option<bool> {
    value.pointer(pointer).and_then(|value| value.as_bool())
}

pub(super) fn json_u64_at(value: &Value, pointer: &str) -> Option<u64> {
    value.pointer(pointer).and_then(|value| value.as_u64())
}

pub(super) fn connect_json_values(bytes: &[u8]) -> Vec<Value> {
    let mut values = Vec::new();
    let mut cursor = 0;
    while cursor + 5 <= bytes.len() {
        let length = u32::from_be_bytes([
            bytes[cursor + 1],
            bytes[cursor + 2],
            bytes[cursor + 3],
            bytes[cursor + 4],
        ]) as usize;
        cursor += 5;
        if cursor + length > bytes.len() {
            values.clear();
            break;
        }
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes[cursor..cursor + length]) {
            values.push(value);
        }
        cursor += length;
    }
    if values.is_empty() {
        values.extend(
            String::from_utf8_lossy(bytes)
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok()),
        );
    }
    values
}
