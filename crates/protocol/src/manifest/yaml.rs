// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

fn yaml_value_to_json_string(value: serde_yaml::Value) -> Result<String> {
    if matches!(value, serde_yaml::Value::Null) {
        return Ok(String::new());
    }
    let json = serde_json::to_value(value).context("Failed to convert YAML value to JSON")?;
    serde_json::to_string(&json).context("Failed to serialize YAML value as JSON")
}

fn json_string_to_yaml_value(value: &str) -> Result<serde_yaml::Value> {
    if value.trim().is_empty() {
        return Ok(serde_yaml::Value::Null);
    }
    let json: serde_json::Value =
        serde_json::from_str(value).context("Failed to parse stored JSON value")?;
    serde_yaml::to_value(json).context("Failed to convert JSON value to YAML")
}
