// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use chrono::SecondsFormat;
use minijinja::{Environment, UndefinedBehavior};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const CLI_PASSTHROUGH_NAMESPACES: &[&str] =
    &["namespace", "deployment", "template", "talon", "ctx"];
const RESOURCE_PASSTHROUGH_NAMESPACES: &[&str] = &["talon", "ctx"];

pub fn render_cli_manifest_template(
    source: &str,
    vars: &HashMap<String, String>,
) -> Result<String> {
    render_phase_template(
        "manifest",
        source,
        json!({ "vars": vars }),
        CLI_PASSTHROUGH_NAMESPACES,
    )
    .context("Failed to render manifest template")
}

pub fn render_resource_template(source: &str, context: Value) -> Result<String> {
    render_phase_template(
        "resource_template",
        source,
        context,
        RESOURCE_PASSTHROUGH_NAMESPACES,
    )
    .context("Failed to render resource template")
}

pub fn render_runtime_system_prompt_template(system_prompt: &str) -> Result<String> {
    if !contains_template_syntax(system_prompt) {
        return Ok(system_prompt.to_string());
    }

    render_runtime_talon_template("system_prompt", system_prompt)
        .context("Failed to render system prompt template")
}

pub fn render_runtime_post_history_prompt_template(post_history_prompt: &str) -> Result<String> {
    if !contains_template_syntax(post_history_prompt) {
        return Ok(post_history_prompt.to_string());
    }

    render_runtime_talon_template("post_history_prompt", post_history_prompt)
        .context("Failed to render post-history prompt template")
}

fn render_runtime_talon_template(template_name: &str, source: &str) -> Result<String> {
    let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    render_strict_template(
        template_name,
        source,
        json!({
            "talon": {
                "now": now,
            },
        }),
    )
}

fn render_phase_template(
    template_name: &str,
    source: &str,
    context: Value,
    passthrough_namespaces: &[&str],
) -> Result<String> {
    if !contains_template_syntax(source) {
        return Ok(source.to_string());
    }

    // Each phase owns only part of the shared `{{ ... }}` namespace. For
    // example, CLI rendering should resolve `vars.*` but must leave
    // `namespace.*` and `talon.*` intact for deployment/runtime phases. Before
    // handing the template to MiniJinja, rewrite those later-phase variable
    // tags into MiniJinja string literals. MiniJinja then emits the original
    // `{{ ... }}` tag as text, so there is no post-render placeholder restore
    // step that could accidentally rewrite a value produced by this phase.
    let passthrough_namespaces = passthrough_namespaces
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let masked = literalize_passthrough_variables(source, &passthrough_namespaces);

    render_strict_template(template_name, &masked, context)
}

fn render_strict_template(template_name: &str, source: &str, context: Value) -> Result<String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.add_template(template_name, source)
        .with_context(|| format!("Failed to compile {} template", template_name))?;
    env.get_template(template_name)
        .with_context(|| format!("Missing {} template", template_name))?
        .render(context)
        .with_context(|| format!("Failed to render {} template", template_name))
}

fn literalize_passthrough_variables(
    source: &str,
    passthrough_namespaces: &HashSet<&str>,
) -> String {
    if passthrough_namespaces.is_empty() {
        return source.to_string();
    }

    let mut output = String::with_capacity(source.len());
    let mut rest = source;

    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let tag_start = &rest[start..];
        let Some(end) = tag_start.find("}}") else {
            output.push_str(tag_start);
            return output;
        };
        let tag_end = start + end + 2;
        let tag = &rest[start..tag_end];
        if variable_namespace(tag)
            .as_deref()
            .is_some_and(|namespace| passthrough_namespaces.contains(namespace))
        {
            output.push_str("{{ ");
            output.push_str(&serde_json::to_string(tag).expect("string serialization cannot fail"));
            output.push_str(" }}");
        } else {
            output.push_str(tag);
        }
        rest = &rest[tag_end..];
    }

    output.push_str(rest);
    output
}

fn variable_namespace(tag: &str) -> Option<String> {
    let inner = tag.strip_prefix("{{")?.strip_suffix("}}")?;
    let expression = inner
        .trim()
        .strip_prefix('-')
        .unwrap_or_else(|| inner.trim())
        .trim()
        .strip_suffix('-')
        .unwrap_or_else(|| {
            inner
                .trim()
                .strip_prefix('-')
                .unwrap_or_else(|| inner.trim())
                .trim()
        })
        .trim();

    let mut chars = expression.chars();
    let first = chars.next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }

    let mut namespace = String::from(first);
    for ch in chars {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            namespace.push(ch);
        } else {
            break;
        }
    }
    Some(namespace)
}

fn contains_template_syntax(source: &str) -> bool {
    source.contains("{{") || source.contains("{%") || source.contains("{#")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_renders_vars_and_preserves_later_phase_variables() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "coding".to_string());

        let rendered = render_cli_manifest_template(
            "name: {{ vars.name }}\nagent: {{ namespace.name }}\nnow: {{ talon.now }}",
            &vars,
        )
        .unwrap();

        assert!(rendered.contains("name: coding"));
        assert!(rendered.contains("agent: {{ namespace.name }}"));
        assert!(rendered.contains("now: {{ talon.now }}"));
    }

    #[test]
    fn cli_fails_on_missing_owned_variable() {
        let err = render_cli_manifest_template("name: {{ vars.missing }}", &HashMap::new())
            .expect_err("missing vars should fail");

        assert!(err
            .to_string()
            .contains("Failed to render manifest template"));
    }

    #[test]
    fn cli_fails_on_unknown_namespace() {
        let err = render_cli_manifest_template("name: {{ nope.value }}", &HashMap::new())
            .expect_err("unknown namespaces should fail");

        assert!(err
            .to_string()
            .contains("Failed to render manifest template"));
    }

    #[test]
    fn cli_preserves_trimmed_filtered_repeated_and_adjacent_later_phase_variables() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "coding".to_string());

        let rendered = render_cli_manifest_template(
            concat!(
                "{{ vars.name }}:",
                "{{- namespace.name -}}",
                "{{ talon.now | upper }}",
                "{{ ctx.agent.name }}",
                ":{{ talon.now }}"
            ),
            &vars,
        )
        .unwrap();

        assert_eq!(
            rendered,
            concat!(
                "coding:",
                "{{- namespace.name -}}",
                "{{ talon.now | upper }}",
                "{{ ctx.agent.name }}",
                ":{{ talon.now }}"
            )
        );
    }

    #[test]
    fn cli_fails_on_later_phase_control_blocks() {
        let err = render_cli_manifest_template("{% if talon.now %}now{% endif %}", &HashMap::new())
            .expect_err("control blocks are not preserved for later phases");

        assert!(err
            .to_string()
            .contains("Failed to render manifest template"));
    }

    #[test]
    fn cli_fails_on_malformed_variable_tags() {
        let err = render_cli_manifest_template("now: {{ talon.now", &HashMap::new())
            .expect_err("malformed variables should fail");

        assert!(err
            .to_string()
            .contains("Failed to render manifest template"));
    }

    #[test]
    fn resource_renders_owned_context_and_preserves_runtime_variables() {
        let rendered = render_resource_template(
            "name: {{ namespace.customerName }}\nnow: {{ talon.now }}",
            json!({
                "namespace": {
                    "customerName": "Acme",
                },
            }),
        )
        .unwrap();

        assert!(rendered.contains("name: Acme"));
        assert!(rendered.contains("now: {{ talon.now }}"));
    }

    #[test]
    fn resource_fails_on_missing_owned_field() {
        let err = render_resource_template(
            "name: {{ namespace.customerName }}",
            json!({ "namespace": {} }),
        )
        .expect_err("missing namespace field should fail");

        assert!(err
            .to_string()
            .contains("Failed to render resource template"));
    }

    #[test]
    fn resource_preserves_adjacent_trimmed_and_filtered_runtime_variables() {
        let rendered = render_resource_template(
            concat!(
                "{{ namespace.customerName }}:",
                "{{- talon.now -}}",
                "{{ ctx.agent.name }}",
                "{{ talon.now | upper }}"
            ),
            json!({
                "namespace": {
                    "customerName": "Acme",
                },
            }),
        )
        .unwrap();

        assert_eq!(
            rendered,
            concat!(
                "Acme:",
                "{{- talon.now -}}",
                "{{ ctx.agent.name }}",
                "{{ talon.now | upper }}"
            )
        );
    }

    #[test]
    fn resource_fails_on_unknown_namespace_and_runtime_control_blocks() {
        for source in ["{{ random.value }}", "{% if talon.now %}now{% endif %}"] {
            render_resource_template(source, json!({ "namespace": {} }))
                .expect_err("resource templates should reject unsupported expressions");
        }
    }

    #[test]
    fn passthrough_literalization_does_not_replace_user_sentinel_text() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "coding".to_string());

        let rendered = render_cli_manifest_template(
            "__TALON_TEMPLATE_PASSTHROUGH_0__ {{ talon.now }} {{ vars.name }}",
            &vars,
        )
        .unwrap();

        assert_eq!(
            rendered,
            "__TALON_TEMPLATE_PASSTHROUGH_0__ {{ talon.now }} coding"
        );
    }

    #[test]
    fn passthrough_literalization_does_not_rewrite_owned_value_matching_old_sentinel() {
        let source = "{{ talon.now }} {{ vars.name }}";
        let old_deterministic_sentinel =
            format!("__TALON_TEMPLATE_PASSTHROUGH_{}0_0__", source.len());
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), old_deterministic_sentinel.clone());

        let rendered = render_cli_manifest_template(source, &vars).unwrap();

        assert_eq!(
            rendered,
            format!("{{{{ talon.now }}}} {old_deterministic_sentinel}")
        );
    }

    #[test]
    fn runtime_renders_talon_now_as_utc_rfc3339_seconds() {
        let rendered = render_runtime_system_prompt_template("Now: {{ talon.now }}").unwrap();
        let timestamp = rendered.strip_prefix("Now: ").unwrap();

        assert!(timestamp.ends_with('Z'));
        assert_eq!(timestamp.len(), "2026-07-03T21:37:12Z".len());
        chrono::DateTime::parse_from_rfc3339(timestamp).unwrap();
    }

    #[test]
    fn runtime_renders_post_history_prompt_talon_now() {
        let rendered =
            render_runtime_post_history_prompt_template("Post: {{ talon.now }}").unwrap();
        let timestamp = rendered.strip_prefix("Post: ").unwrap();

        assert!(timestamp.ends_with('Z'));
        assert_eq!(timestamp.len(), "2026-07-03T21:37:12Z".len());
        chrono::DateTime::parse_from_rfc3339(timestamp).unwrap();
    }

    #[test]
    fn runtime_leaves_static_prompt_unchanged() {
        let prompt = "Answer like the configured agent.";

        assert_eq!(
            render_runtime_system_prompt_template(prompt).unwrap(),
            prompt
        );
    }

    #[test]
    fn runtime_supports_trimmed_and_filtered_talon_now() {
        let rendered = render_runtime_system_prompt_template("{{- talon.now | upper -}}").unwrap();

        assert!(rendered.ends_with('Z'));
        chrono::DateTime::parse_from_rfc3339(&rendered).unwrap();
    }

    #[test]
    fn runtime_fails_on_non_runtime_namespaces_and_unknown_talon_fields() {
        for prompt in [
            "{{ namespace.name }}",
            "{{ ctx.agent.name }}",
            "{{ talon.nope }}",
        ] {
            render_runtime_system_prompt_template(prompt)
                .expect_err("runtime prompt should reject unsupported variables");
        }
    }

    #[test]
    fn runtime_post_history_prompt_fails_on_unknown_variables() {
        let err = render_runtime_post_history_prompt_template("{{ talon.nope }}")
            .expect_err("unknown post-history prompt variables should fail");

        assert!(err
            .to_string()
            .contains("Failed to render post-history prompt template"));
    }

    #[test]
    fn runtime_supports_minijinja_literal_escaping() {
        let rendered =
            render_runtime_system_prompt_template(r#"Literal: {{ "{{ talon.now }}" }}"#).unwrap();

        assert_eq!(rendered, "Literal: {{ talon.now }}");
    }
}
