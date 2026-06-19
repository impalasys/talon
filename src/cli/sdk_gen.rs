// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '-' || c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

pub(super) fn sdk_methods_from_dir(dir: &str) -> Result<Vec<String>> {
    let mut class_methods = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().unwrap_or_default() != "yaml" {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        let raw = parse_raw_manifest(&content)?;
        if raw.kind == "Agent" {
            let agent = crate::control::manifest::parse_agent(&content)?;
            let method_name = format!("create{}", to_camel_case(&agent.name()));
            class_methods.push(format!(
                r#"  async {method_name}(workspaceId: string): Promise<any> {{
    return fetch(`${{this.endpoint}}/api/agents`, {{
      method: "POST",
      headers: {{ "Content-Type": "application/json" }},
      body: JSON.stringify({{ agent: "{raw_name}", namespace: workspaceId, inputs: {{}} }})
    }}).then(r => r.json());
  }}"#,
                method_name = method_name,
                raw_name = agent.name(),
            ));
        }
    }
    Ok(class_methods)
}
