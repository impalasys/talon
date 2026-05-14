/// Generates the standard KeyValue store path for an Agent
/// e.g., "Agent/billing-bot-abc123"
pub fn agent(id: &str) -> String {
    format!("Agent/{}", id)
}

/// Generates the standard KeyValue store path for an Agent Session
/// e.g., "Agent/billing-bot-abc123/Session/XYZ"
pub fn session(agent: &str, session_id: &str) -> String {
    format!("Agent/{}/Session/{}", agent, session_id)
}

/// Generates the standard KeyValue store path for searching an Agent's Sessions prefix
pub fn session_prefix(agent: &str) -> String {
    format!("Agent/{}/Session/", agent)
}

/// Generates the key for a single message inside a Session
pub fn session_message(agent: &str, session_id: &str, message_id: &str) -> String {
    format!(
        "Agent/{}/Session/{}/Messages/{}",
        agent, session_id, message_id
    )
}

/// Generates the key prefix to list all messages safely sequentially sorted temporally
pub fn session_message_prefix(agent: &str, session_id: &str) -> String {
    format!("Agent/{}/Session/{}/Messages/", agent, session_id)
}

/// Generates the key for a single step inside a SessionMessage
pub fn session_message_step(
    agent: &str,
    session_id: &str,
    message_id: &str,
    step_id: &str,
) -> String {
    format!(
        "Agent/{}/Session/{}/Messages/{}/Steps/{}",
        agent, session_id, message_id, step_id
    )
}

/// Generates the key prefix to list all steps for a single message
pub fn session_message_step_prefix(agent: &str, session_id: &str, message_id: &str) -> String {
    format!(
        "Agent/{}/Session/{}/Messages/{}/Steps/",
        agent, session_id, message_id
    )
}

/// Generates the key for a schedule resource within a namespace.
pub fn schedule(name: &str) -> String {
    format!("Schedule/{}", name)
}

/// Prefix used to list all schedule resources in a namespace.
pub fn schedule_prefix() -> &'static str {
    "Schedule/"
}

/// Generates the standard KeyValue store path for an AgentTemplate
/// e.g., "AgentTemplate/customer-service"
pub fn agent_template(name: &str) -> String {
    format!("AgentTemplate/{}", name)
}

/// Generates the key for an MCP server resource.
pub fn mcp_server(name: &str) -> String {
    format!("McpServer/{}", name)
}

/// Generates the prefix used to list MCP server resources in a namespace.
pub fn mcp_server_prefix() -> &'static str {
    "McpServer/"
}

/// Generates the key for an MCP server binding resource in a namespace.
pub fn mcp_server_binding(name: &str) -> String {
    format!("McpServerBinding/{}", name)
}

/// Generates the prefix used to list MCP server bindings in a namespace.
pub fn mcp_server_binding_prefix() -> &'static str {
    "McpServerBinding/"
}

/// Generates the key for a memory file in an agent's storage
/// e.g. "Agent/seo-agent/Memory/goals.md"
pub fn agent_memory(agent: &str, path: &str) -> String {
    format!("Agent/{}/Memory/{}", agent, path)
}

/// Generates the key prefix to list all memory files for an agent
/// e.g. "Agent/seo-agent/Memory/"
pub fn agent_memory_prefix(agent: &str) -> String {
    format!("Agent/{}/Memory/", agent)
}

/// Key for a KnowledgeBook artifact within a namespace.
/// Usage: kv.get(ns, &keys::knowledge(path))
/// e.g.   kv.get("acme-corp", &keys::knowledge("seo/best-practices.md"))
///        => full logical path: acme-corp / Knowledge/seo/best-practices.md
pub fn knowledge(path: &str) -> String {
    format!("Knowledge/{}", path)
}

/// Prefix to list all KnowledgeBook artifacts for a namespace.
pub fn knowledge_prefix() -> &'static str {
    "Knowledge/"
}

/// Generates the key for a manifest-managed knowledge resource in a namespace.
pub fn knowledge_resource(name: &str) -> String {
    format!("KnowledgeResource/{}", name)
}

/// Prefix used to list manifest-managed knowledge resources in a namespace.
pub fn knowledge_resource_prefix() -> &'static str {
    "KnowledgeResource/"
}
