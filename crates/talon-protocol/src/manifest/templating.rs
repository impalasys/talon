// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use chrono::SecondsFormat;
use minijinja::machinery::{ast, parse_expr, tokenize, Span, Token, WhitespaceConfig};
use minijinja::syntax::SyntaxConfig;
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

    let passthrough_namespaces = passthrough_namespaces
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let masked = literalize_passthrough_variables(source, &passthrough_namespaces)
        .with_context(|| format!("Failed to prepare {} template", template_name))?;

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
) -> Result<String> {
    if passthrough_namespaces.is_empty() {
        return Ok(source.to_string());
    }

    let mut output = String::with_capacity(source.len());
    let mut copied_until = 0;
    let mut variable_start: Option<Span> = None;

    // Each phase owns only part of Talon's shared `{{ ... }}` namespace. For
    // example, CLI rendering resolves `vars.*` but leaves `talon.*` for the
    // runtime phase. We use MiniJinja's tokenizer to locate exact variable tag
    // byte spans and its expression parser to decide whether the whole tag is
    // rooted in a later-owned namespace. Preserved tags are rewritten as
    // MiniJinja string literals before the current phase renders, so MiniJinja
    // emits the original `{{ ... }}` text and we never need a fragile
    // post-render placeholder replacement step.
    for token in tokenize(
        source,
        false,
        SyntaxConfig::default(),
        WhitespaceConfig::default(),
    ) {
        let (token, span) = token?;
        match token {
            Token::VariableStart => variable_start = Some(span),
            Token::VariableEnd => {
                let Some(start_span) = variable_start.take() else {
                    continue;
                };
                let tag_start = start_span.start_offset as usize;
                let tag_end = span.end_offset as usize;
                let expression_start = start_span.end_offset as usize;
                let expression_end = span.start_offset as usize;
                let expression =
                    trim_variable_expression(&source[expression_start..expression_end]);
                let expr = parse_expr(expression)?;

                if is_passthrough_expression(&expr, passthrough_namespaces) {
                    let tag = &source[tag_start..tag_end];
                    output.push_str(&source[copied_until..tag_start]);
                    output.push_str("{{ ");
                    output.push_str(
                        &serde_json::to_string(tag).expect("string serialization cannot fail"),
                    );
                    output.push_str(" }}");
                    copied_until = tag_end;
                }
            }
            _ => {}
        }
    }

    output.push_str(&source[copied_until..]);
    Ok(output)
}

fn trim_variable_expression(expression: &str) -> &str {
    let mut expression = expression.trim();
    if let Some(stripped) = expression.strip_prefix('-') {
        expression = stripped.trim();
    }
    if let Some(stripped) = expression.strip_suffix('-') {
        expression = stripped.trim();
    }
    expression
}

fn is_passthrough_expression(expr: &ast::Expr<'_>, passthrough_namespaces: &HashSet<&str>) -> bool {
    root_namespace(expr).is_some_and(|namespace| passthrough_namespaces.contains(namespace))
        && all_variable_roots_are_passthrough(expr, passthrough_namespaces)
}

fn root_namespace<'a>(expr: &'a ast::Expr<'a>) -> Option<&'a str> {
    match expr {
        ast::Expr::Var(var) => Some(var.id),
        ast::Expr::Const(_) => None,
        ast::Expr::Slice(slice) => root_namespace(&slice.expr)
            .or_else(|| slice.start.as_ref().and_then(root_namespace))
            .or_else(|| slice.stop.as_ref().and_then(root_namespace))
            .or_else(|| slice.step.as_ref().and_then(root_namespace)),
        ast::Expr::UnaryOp(unary) => root_namespace(&unary.expr),
        ast::Expr::BinOp(bin) => root_namespace(&bin.left).or_else(|| root_namespace(&bin.right)),
        ast::Expr::IfExpr(if_expr) => root_namespace(&if_expr.test_expr)
            .or_else(|| root_namespace(&if_expr.true_expr))
            .or_else(|| if_expr.false_expr.as_ref().and_then(root_namespace)),
        ast::Expr::Filter(filter) => filter
            .expr
            .as_ref()
            .and_then(root_namespace)
            .or_else(|| filter.args.iter().find_map(call_arg_root_namespace)),
        ast::Expr::Test(test) => root_namespace(&test.expr)
            .or_else(|| test.args.iter().find_map(call_arg_root_namespace)),
        ast::Expr::GetAttr(get_attr) => root_namespace(&get_attr.expr),
        ast::Expr::GetItem(get_item) => root_namespace(&get_item.expr),
        ast::Expr::Call(call) => root_namespace(&call.expr),
        ast::Expr::List(list) => list.items.iter().find_map(root_namespace),
        ast::Expr::Map(map) => map
            .keys
            .iter()
            .find_map(root_namespace)
            .or_else(|| map.values.iter().find_map(root_namespace)),
    }
}

fn call_arg_root_namespace<'a>(arg: &'a ast::CallArg<'a>) -> Option<&'a str> {
    match arg {
        ast::CallArg::Pos(expr)
        | ast::CallArg::Kwarg(_, expr)
        | ast::CallArg::PosSplat(expr)
        | ast::CallArg::KwargSplat(expr) => root_namespace(expr),
    }
}

fn all_variable_roots_are_passthrough(
    expr: &ast::Expr<'_>,
    passthrough_namespaces: &HashSet<&str>,
) -> bool {
    match expr {
        ast::Expr::Var(var) => passthrough_namespaces.contains(var.id),
        ast::Expr::Const(_) => true,
        ast::Expr::Slice(slice) => {
            all_variable_roots_are_passthrough(&slice.expr, passthrough_namespaces)
                && slice.start.as_ref().is_none_or(|expr| {
                    all_variable_roots_are_passthrough(expr, passthrough_namespaces)
                })
                && slice.stop.as_ref().is_none_or(|expr| {
                    all_variable_roots_are_passthrough(expr, passthrough_namespaces)
                })
                && slice.step.as_ref().is_none_or(|expr| {
                    all_variable_roots_are_passthrough(expr, passthrough_namespaces)
                })
        }
        ast::Expr::UnaryOp(unary) => {
            all_variable_roots_are_passthrough(&unary.expr, passthrough_namespaces)
        }
        ast::Expr::BinOp(bin) => {
            all_variable_roots_are_passthrough(&bin.left, passthrough_namespaces)
                && all_variable_roots_are_passthrough(&bin.right, passthrough_namespaces)
        }
        ast::Expr::IfExpr(if_expr) => {
            all_variable_roots_are_passthrough(&if_expr.test_expr, passthrough_namespaces)
                && all_variable_roots_are_passthrough(&if_expr.true_expr, passthrough_namespaces)
                && if_expr.false_expr.as_ref().is_none_or(|expr| {
                    all_variable_roots_are_passthrough(expr, passthrough_namespaces)
                })
        }
        ast::Expr::Filter(filter) => {
            filter
                .expr
                .as_ref()
                .is_none_or(|expr| all_variable_roots_are_passthrough(expr, passthrough_namespaces))
                && filter
                    .args
                    .iter()
                    .all(|arg| call_arg_roots_are_passthrough(arg, passthrough_namespaces))
        }
        ast::Expr::Test(test) => {
            all_variable_roots_are_passthrough(&test.expr, passthrough_namespaces)
                && test
                    .args
                    .iter()
                    .all(|arg| call_arg_roots_are_passthrough(arg, passthrough_namespaces))
        }
        ast::Expr::GetAttr(get_attr) => {
            all_variable_roots_are_passthrough(&get_attr.expr, passthrough_namespaces)
        }
        ast::Expr::GetItem(get_item) => {
            all_variable_roots_are_passthrough(&get_item.expr, passthrough_namespaces)
                && all_variable_roots_are_passthrough(
                    &get_item.subscript_expr,
                    passthrough_namespaces,
                )
        }
        ast::Expr::Call(call) => {
            all_variable_roots_are_passthrough(&call.expr, passthrough_namespaces)
                && call
                    .args
                    .iter()
                    .all(|arg| call_arg_roots_are_passthrough(arg, passthrough_namespaces))
        }
        ast::Expr::List(list) => list
            .items
            .iter()
            .all(|expr| all_variable_roots_are_passthrough(expr, passthrough_namespaces)),
        ast::Expr::Map(map) => {
            map.keys
                .iter()
                .all(|expr| all_variable_roots_are_passthrough(expr, passthrough_namespaces))
                && map
                    .values
                    .iter()
                    .all(|expr| all_variable_roots_are_passthrough(expr, passthrough_namespaces))
        }
    }
}

fn call_arg_roots_are_passthrough(
    arg: &ast::CallArg<'_>,
    passthrough_namespaces: &HashSet<&str>,
) -> bool {
    match arg {
        ast::CallArg::Pos(expr)
        | ast::CallArg::Kwarg(_, expr)
        | ast::CallArg::PosSplat(expr)
        | ast::CallArg::KwargSplat(expr) => {
            all_variable_roots_are_passthrough(expr, passthrough_namespaces)
        }
    }
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
    fn cli_preserves_parser_backed_later_phase_expression_shapes() {
        let rendered = render_cli_manifest_template(
            concat!(
                r#"{{ talon["now"] }}"#,
                "{{ talon.now is string }}",
                "{{ talon.format('prefix', ctx.agent.name) }}",
                "{{ namespace.customerName | default('Acme') }}",
                "{{ talon.now ~ ' UTC' }}",
                "{{ not talon.now }}",
                "{{ talon.now if true else ctx.agent.name }}",
                "{{ [talon.now, ctx.agent.name] }}"
            ),
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(
            rendered,
            concat!(
                r#"{{ talon["now"] }}"#,
                "{{ talon.now is string }}",
                "{{ talon.format('prefix', ctx.agent.name) }}",
                "{{ namespace.customerName | default('Acme') }}",
                "{{ talon.now ~ ' UTC' }}",
                "{{ not talon.now }}",
                "{{ talon.now if true else ctx.agent.name }}",
                "{{ [talon.now, ctx.agent.name] }}"
            )
        );
    }

    #[test]
    fn cli_handles_comments_raw_blocks_and_literal_strings_with_template_delimiters() {
        let rendered = render_cli_manifest_template(
            concat!(
                "{# {{ talon.now }} #}",
                "{% raw %}{{ talon.now }}{% endraw %}",
                r#"{{ "{{ talon.now }}" }}"#,
                "{{ talon.now }}"
            ),
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(rendered, "{{ talon.now }}{{ talon.now }}{{ talon.now }}");
    }

    #[test]
    fn cli_fails_on_mixed_current_and_later_phase_expressions() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), String::new());

        for source in [
            "{{ talon.now ~ vars.name }}",
            "{{ vars.name or talon.now }}",
            "{{ talon.format(vars.name) }}",
            "{{ [talon.now, vars.name] }}",
            "{{ talon.now if vars.name else ctx.agent.name }}",
        ] {
            render_cli_manifest_template(source, &vars)
                .expect_err("mixed current and later phase expressions should fail");
        }
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
    fn resource_preserves_parser_backed_runtime_expression_shapes() {
        let rendered = render_resource_template(
            concat!(
                r#"{{ talon["now"] }}"#,
                "{{ talon.now is string }}",
                "{{ talon.format('prefix', ctx.agent.name) }}",
                "{{ talon.now ~ ' UTC' }}",
                "{{ not talon.now }}"
            ),
            json!({ "namespace": {} }),
        )
        .unwrap();

        assert_eq!(
            rendered,
            concat!(
                r#"{{ talon["now"] }}"#,
                "{{ talon.now is string }}",
                "{{ talon.format('prefix', ctx.agent.name) }}",
                "{{ talon.now ~ ' UTC' }}",
                "{{ not talon.now }}"
            )
        );
    }

    #[test]
    fn resource_fails_on_mixed_resource_and_runtime_expressions() {
        let context = json!({
            "namespace": {
                "customerName": "Acme",
            },
        });

        for source in [
            "{{ talon.now ~ namespace.customerName }}",
            "{{ talon.format(namespace.customerName) }}",
            "{{ [talon.now, namespace.customerName] }}",
        ] {
            render_resource_template(source, context.clone())
                .expect_err("mixed resource and runtime expressions should fail");
        }
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
