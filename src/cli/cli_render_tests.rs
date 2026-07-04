// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(test)]
mod cli_render_tests {
    use super::*;

    #[test]
    fn render_manifest_template_renders_vars_and_preserves_later_layer_vars() {
        let mut vars = HashMap::new();
        vars.insert("source_ns".to_string(), "customers".to_string());
        let rendered = render_cli_manifest_template(
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
      You are the coding agent for {{ namespace.name }}.
      Current Talon time: {{ talon.now }}.
"#,
            &vars,
        )
        .expect("template renders");

        assert!(rendered.contains("namespace: \"customers\""));
        assert!(rendered.contains("{{ namespace.name }}"));
        assert!(rendered.contains("{{ talon.now }}"));
    }

    #[test]
    fn render_manifest_template_fails_on_undefined_vars() {
        let vars = HashMap::new();
        render_cli_manifest_template("name: {{ vars.missing }}", &vars)
            .expect_err("undefined var should fail");
    }

    #[test]
    fn render_manifest_file_allows_empty_rendered_template() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("optional.yaml");
        std::fs::write(
            &path,
            r#"{% if vars.enabled is defined and vars.enabled == "1" %}
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: optional
{% endif %}
"#,
        )
        .expect("write manifest");

        let rendered = render_manifest_file(path.to_str().unwrap(), &[]).expect("render manifest");

        assert!(rendered.trim().is_empty());
    }

    #[test]
    fn render_manifest_file_resolves_content_from_file_in_multi_document_yaml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let knowledge_path = dir.path().join("guide.md");
        std::fs::write(&knowledge_path, "hello from file").expect("write knowledge");
        let manifest_path = dir.path().join("stack.yaml");
        std::fs::write(
            &manifest_path,
            r#"
apiVersion: talon.impalasys.com/v1
kind: Namespace
metadata:
  name: demo
---
apiVersion: talon.impalasys.com/v1
kind: Knowledge
metadata:
  namespace: demo
  name: guide
spec:
  path: guide.md
  contentFromFile: guide.md
"#,
        )
        .expect("write manifest");

        let rendered =
            render_manifest_file(manifest_path.to_str().unwrap(), &[]).expect("render manifest");

        assert!(rendered.contains("content: hello from file"));
        assert!(!rendered.contains("contentFromFile"));
        assert!(!rendered.contains("---\n---"));
    }

    #[test]
    fn resolve_manifest_sources_allows_non_mapping_documents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let knowledge_path = dir.path().join("guide.md");
        std::fs::write(&knowledge_path, "hello from file").expect("write knowledge");
        let manifest_path = dir.path().join("stack.yaml");
        std::fs::write(&manifest_path, "").expect("write manifest");

        let rendered = resolve_manifest_sources(
            manifest_path.to_str().unwrap(),
            r#"
- just
- a
- list
---
apiVersion: talon.impalasys.com/v1
kind: Knowledge
metadata:
  namespace: demo
  name: guide
spec:
  path: guide.md
  contentFromFile: guide.md
"#,
        )
        .expect("resolve sources");

        assert!(rendered.contains("- just"));
        assert!(rendered.contains("content: hello from file"));
    }
}
