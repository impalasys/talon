// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

// Unified tool registry merging MCP + built-in + skill tools
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::connectors::mcp::McpTool;
use crate::skills::loader::Skill;

#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub source: ToolSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolSource {
    Builtin,
    Mcp(String),   // server name
    Skill(String), // skill name
}

pub struct ToolRegistry {
    pub tools: HashMap<String, ToolDefinition>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register_builtin(&mut self, name: &str, description: &str, schema: Value) {
        self.tools.insert(
            name.to_string(),
            ToolDefinition {
                name: name.to_string(),
                description: description.to_string(),
                input_schema: schema,
                source: ToolSource::Builtin,
            },
        );
    }

    pub fn register_mcp_tools(&mut self, server_name: &str, tools: Vec<McpTool>) {
        for tool in tools {
            self.tools.insert(
                tool.name.clone(),
                ToolDefinition {
                    name: tool.name.clone(),
                    description: tool.description,
                    input_schema: tool.input_schema,
                    source: ToolSource::Mcp(server_name.to_string()),
                },
            );
        }
    }

    pub fn register_skills(&mut self, skills: Vec<Skill>) {
        for skill in skills {
            // Skills are registered as tools with a single 'query' or 'input' parameter?
            // Actually, requirements say "register each skill as a tool".
            // Skill has 'instructions' which is the prompt for the skill.
            // A skill tool might just take an 'input' string.
            let schema = json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "Input for the skill"
                    }
                },
                "required": ["input"]
            });

            self.tools.insert(
                skill.name.clone(),
                ToolDefinition {
                    name: skill.name.clone(),
                    description: skill.description,
                    input_schema: schema,
                    source: ToolSource::Skill(skill.name),
                },
            );
        }
    }

    pub fn get_tool(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name)
    }

    pub fn list_tools(&self) -> Vec<&ToolDefinition> {
        self.tools.values().collect()
    }

    pub fn to_provider_tools(&self) -> Vec<crate::llm::provider::Tool> {
        self.tools
            .values()
            .map(|t| crate::llm::provider::Tool {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_get_builtin() {
        let mut registry = ToolRegistry::new();
        let schema = json!({ "type": "object" });
        registry.register_builtin("test_tool", "A test tool", schema.clone());

        let tool = registry.get_tool("test_tool").unwrap();
        assert_eq!(tool.name, "test_tool");
        assert_eq!(tool.description, "A test tool");
        assert_eq!(tool.input_schema, schema);
        assert_eq!(tool.source, ToolSource::Builtin);
    }

    #[test]
    fn test_list_tools() {
        let mut registry = ToolRegistry::new();
        registry.register_builtin("tool1", "desc1", json!({}));
        registry.register_builtin("tool2", "desc2", json!({}));
        registry.register_builtin("tool3", "desc3", json!({}));

        let tools = registry.list_tools();
        assert_eq!(tools.len(), 3);
    }

    #[test]
    fn test_get_tool_not_found() {
        let registry = ToolRegistry::new();
        assert!(registry.get_tool("unknown").is_none());
    }

    #[test]
    fn test_to_provider_tools() {
        let mut registry = ToolRegistry::new();
        let schema = json!({
            "type": "object",
            "properties": {
                "arg": { "type": "string" }
            }
        });
        registry.register_builtin("test_tool", "desc", schema.clone());

        let tools = registry.to_provider_tools();
        assert_eq!(tools.len(), 1);
        let tool = &tools[0];
        assert_eq!(tool.name, "test_tool");
        assert_eq!(tool.description, "desc");
        assert_eq!(tool.input_schema, schema);
    }

    #[test]
    fn test_register_knowledge_tools() {
        let mut registry = ToolRegistry::new();
        crate::knowledge::register_tools(&mut registry);

        assert!(registry
            .get_tool(crate::knowledge::KNOWLEDGE_WRITE_TOOL)
            .is_some());
        assert!(registry
            .get_tool(crate::knowledge::KNOWLEDGE_SEARCH_TOOL)
            .is_some());
        assert!(registry
            .get_tool(crate::knowledge::KNOWLEDGE_GET_TOOL)
            .is_some());
    }
}
