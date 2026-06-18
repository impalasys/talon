#[cfg(test)]
mod cli_render_tests {
    use super::*;

    #[test]
    fn render_manifest_template_renders_template_outer_vars_and_preserves_raw_inner_vars() {
        let mut vars = HashMap::new();
        vars.insert("source_ns".to_string(), "customers".to_string());
        let rendered = render_manifest_template(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Template
metadata:
  name: coding-agent
  namespace: "{{ vars.source_ns }}"
spec:
  kind: Agent
  spec:
    systemPrompt: |
      You are the coding agent for {% raw %}{{ namespace.name }}{% endraw %}.
"#,
            &vars,
        )
        .expect("template renders");

        assert!(rendered.contains("namespace: \"customers\""));
        assert!(rendered.contains("{{ namespace.name }}"));
    }

    #[test]
    fn render_manifest_template_fails_on_undefined_vars() {
        let vars = HashMap::new();
        render_manifest_template("name: {{ vars.missing }}", &vars)
            .expect_err("undefined var should fail");
    }
}
