// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::{resource_model::TypedResource, resources_proto};
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub fn validate_workflow(workflow: &resources_proto::Workflow) -> Result<()> {
    if workflow.name().trim().is_empty() {
        bail!("workflow name is required");
    }
    if workflow.name().contains('/') {
        bail!("workflow name cannot contain '/'");
    }
    if workflow.namespace().trim().is_empty() {
        bail!("workflow namespace is required");
    }
    let spec = workflow
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("workflow spec is required"))?;
    if spec.steps.is_empty() {
        bail!("workflow spec.steps must contain at least one step");
    }

    validate_schema_json("inputSchema", &spec.input_schema_json)?;
    validate_schema_json("outputSchema", &spec.output_schema_json)?;
    validate_json_object("output", &spec.output_json)?;

    let mut ids = HashSet::new();
    for step in &spec.steps {
        if step.id.trim().is_empty() {
            bail!("workflow step id is required");
        }
        if !ids.insert(step.id.clone()) {
            bail!("duplicate workflow step id '{}'", step.id);
        }
        match step.r#type.as_str() {
            "agent" | "tool" | "workflow" | "transform" | "pause" | "wait" => {}
            other => bail!("unsupported workflow step type '{}'", other),
        }
        match step.r#type.as_str() {
            "agent" => {
                if step.agent.trim().is_empty() {
                    bail!("agent step '{}' requires agent", step.id);
                }
                if step.prompt.trim().is_empty() {
                    bail!("agent step '{}' requires prompt", step.id);
                }
            }
            "tool" if step.tool.trim().is_empty() => {
                bail!("tool step '{}' requires tool", step.id);
            }
            "workflow" if step.workflow.trim().is_empty() => {
                bail!("workflow step '{}' requires workflow", step.id);
            }
            "pause" if step.prompt.trim().is_empty() => {
                bail!("pause step '{}' requires prompt", step.id);
            }
            "wait"
                if step.wait_duration.trim().is_empty()
                    && step.wait_until.trim().is_empty()
                    && step.resume_schema_json.trim().is_empty() =>
            {
                bail!(
                    "wait step '{}' requires duration, until, or resumeSchema",
                    step.id
                );
            }
            _ => {}
        }
        validate_json_object(&format!("step {} when", step.id), &step.when_json)?;
        validate_predicate_json(&format!("step {} when", step.id), &step.when_json)?;
        validate_json_value(&format!("step {} input", step.id), &step.input_json)?;
        validate_schema_json(
            &format!("step {} resumeSchema", step.id),
            &step.resume_schema_json,
        )?;
        validate_duration_field(&format!("step {} timeout", step.id), &step.timeout)?;
        validate_duration_field(&format!("step {} duration", step.id), &step.wait_duration)?;
        if !step.wait_until.trim().is_empty() {
            parse_timestamp_micros(&step.wait_until)
                .map_err(|err| anyhow!("step {} until is invalid: {}", step.id, err))?;
        }
        if let Some(retry) = &step.retry {
            validate_retry_policy(&step.id, retry)?;
        }
        if let Some(output) = &step.output {
            validate_schema_json(
                &format!("step {} output.schema", step.id),
                &output.schema_json,
            )?;
            match output.format.as_str() {
                "" | "text" | "json" => {}
                other => bail!(
                    "unsupported output format '{}' for step '{}'",
                    other,
                    step.id
                ),
            }
        }
        for dep in &step.after {
            if dep == &step.id {
                bail!("workflow step '{}' cannot depend on itself", step.id);
            }
        }
    }
    for step in &spec.steps {
        for dep in &step.after {
            if !ids.contains(dep) {
                bail!(
                    "workflow step '{}' depends on unknown step '{}'",
                    step.id,
                    dep
                );
            }
        }
    }
    detect_cycle(spec)?;
    Ok(())
}

pub fn validate_input(schema_json: &str, input: &Value) -> Result<()> {
    validate_basic_json_schema("input", schema_json, input)
}

pub fn validate_schema_json(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let parsed: Value =
        serde_json::from_str(value).with_context(|| format!("{label} must be valid JSON"))?;
    if !parsed.is_object() {
        bail!("{label} must be a JSON object");
    }
    Ok(())
}

pub fn validate_duration_field(label: &str, value: &str) -> Result<()> {
    parse_duration_seconds(value)
        .map(|_| ())
        .map_err(|err| anyhow!("{label} is invalid: {err}"))
}

pub fn validate_retry_policy(
    step_id: &str,
    retry: &resources_proto::WorkflowStepRetryPolicy,
) -> Result<()> {
    if retry.max_attempts == 0 {
        bail!(
            "retry.maxAttempts for step '{}' must be at least 1",
            step_id
        );
    }
    if retry.initial_backoff_seconds < 0 {
        bail!(
            "retry.initialBackoffSeconds for step '{}' must be non-negative",
            step_id
        );
    }
    if retry.max_backoff_seconds < 0 {
        bail!(
            "retry.maxBackoffSeconds for step '{}' must be non-negative",
            step_id
        );
    }
    if retry.multiplier < 0.0 {
        bail!(
            "retry.multiplier for step '{}' must be non-negative",
            step_id
        );
    }
    Ok(())
}

pub fn parse_timestamp_micros(value: &str) -> Result<i64> {
    let parsed = DateTime::parse_from_rfc3339(value.trim())?;
    Ok(parsed.with_timezone(&Utc).timestamp_micros())
}

pub fn parse_duration_seconds(value: &str) -> Result<Option<i64>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let (number, multiplier) = match value.chars().last() {
        Some('s') => (&value[..value.len() - 1], 1),
        Some('m') => (&value[..value.len() - 1], 60),
        Some('h') => (&value[..value.len() - 1], 60 * 60),
        Some('d') => (&value[..value.len() - 1], 60 * 60 * 24),
        Some(c) if c.is_ascii_digit() => (value, 1),
        _ => bail!("duration must use seconds, s, m, h, or d"),
    };
    let number = number
        .parse::<i64>()
        .map_err(|err| anyhow!("duration value must be an integer: {}", err))?;
    if number <= 0 {
        bail!("duration must be positive");
    }
    Ok(Some(number.saturating_mul(multiplier)))
}

pub fn validate_json_object(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let parsed: Value =
        serde_json::from_str(value).with_context(|| format!("{label} must be valid JSON"))?;
    if !parsed.is_object() {
        bail!("{label} must be a JSON object");
    }
    Ok(())
}

pub fn validate_json_value(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let _: Value =
        serde_json::from_str(value).with_context(|| format!("{label} must be valid JSON"))?;
    Ok(())
}

pub fn validate_predicate_json(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let predicate: Value =
        serde_json::from_str(value).with_context(|| format!("{label} must be valid JSON"))?;
    validate_predicate_shape(label, &predicate)
}

pub fn validate_predicate_shape(label: &str, predicate: &Value) -> Result<()> {
    let Some(object) = predicate.as_object() else {
        bail!("{label} must be an object");
    };
    if let Some(all) = object.get("all") {
        let items = all
            .as_array()
            .ok_or_else(|| anyhow!("{label}.all must be an array"))?;
        for item in items {
            validate_predicate_shape(label, item)?;
        }
        return Ok(());
    }
    if let Some(any) = object.get("any") {
        let items = any
            .as_array()
            .ok_or_else(|| anyhow!("{label}.any must be an array"))?;
        for item in items {
            validate_predicate_shape(label, item)?;
        }
        return Ok(());
    }
    if let Some(not) = object.get("not") {
        return validate_predicate_shape(label, not);
    }

    if !object.get("path").is_some_and(Value::is_string) {
        bail!("{label} predicate requires path");
    }
    let comparators = [
        "exists",
        "equals",
        "notEquals",
        "in",
        "contains",
        "gt",
        "gte",
        "lt",
        "lte",
    ];
    let present = comparators
        .iter()
        .filter(|key| object.contains_key(**key))
        .collect::<Vec<_>>();
    if present.len() != 1 {
        bail!("{label} predicate must set exactly one comparator");
    }
    match *present[0] {
        "exists" if !object.get("exists").is_some_and(Value::is_boolean) => {
            bail!("{label}.exists must be boolean");
        }
        "in" if !object.get("in").is_some_and(Value::is_array) => {
            bail!("{label}.in must be an array");
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_basic_json_schema(label: &str, schema_json: &str, value: &Value) -> Result<()> {
    if schema_json.trim().is_empty() {
        return Ok(());
    }
    let schema: Value = serde_json::from_str(schema_json)?;
    if schema.get("type").and_then(Value::as_str) == Some("object") && !value.is_object() {
        bail!("{label} must be an object");
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for field in required.iter().filter_map(Value::as_str) {
            if value.get(field).is_none() {
                bail!("{label} is missing required property '{field}'");
            }
        }
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (field, property_schema) in properties {
            let Some(field_value) = value.get(field) else {
                continue;
            };
            let Some(type_name) = property_schema.get("type").and_then(Value::as_str) else {
                continue;
            };
            let ok = match type_name {
                "string" => field_value.is_string(),
                "boolean" => field_value.is_boolean(),
                "number" => field_value.is_number(),
                "integer" => field_value.as_i64().is_some() || field_value.as_u64().is_some(),
                "object" => field_value.is_object(),
                "array" => field_value.is_array(),
                _ => true,
            };
            if !ok {
                bail!("{label}.{field} must be {type_name}");
            }
        }
    }
    Ok(())
}

pub fn detect_cycle(spec: &resources_proto::WorkflowSpec) -> Result<()> {
    let graph = spec
        .steps
        .iter()
        .map(|step| {
            (
                step.id.as_str(),
                step.after.iter().map(String::as_str).collect::<Vec<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for step in &spec.steps {
        visit(&step.id, &graph, &mut visiting, &mut visited)?;
    }
    Ok(())
}

pub fn visit<'a>(
    id: &'a str,
    graph: &HashMap<&'a str, Vec<&'a str>>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
) -> Result<()> {
    if visited.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id) {
        bail!("workflow contains a dependency cycle involving '{}'", id);
    }
    for dep in graph.get(id).into_iter().flatten() {
        visit(dep, graph, visiting, visited)?;
    }
    visiting.remove(id);
    visited.insert(id);
    Ok(())
}

trait ContextExt<T> {
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String;
}

impl<T, E> ContextExt<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|err| anyhow!("{}: {}", f(), err))
    }
}
