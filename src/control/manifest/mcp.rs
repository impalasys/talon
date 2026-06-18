#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpServerManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    spec: McpServerSpecManifest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct McpServerSpecManifest {
    transport: String,
    target: String,
    args: Vec<String>,
    headers: HashMap<String, String>,
    disabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    spec: AgentSpecManifest,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpServerBindingManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    spec: McpServerBindingSpecManifest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct McpServerBindingSpecManifest {
    server_ref: String,
    args: Vec<String>,
    headers: HashMap<String, String>,
    disabled: bool,
    auth_broker: Option<McpAuthBrokerSpecManifest>,
    allowed_tool_names: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct A2AManifest {
    connections: Vec<ConnectionManifest>,
    agent_card: Option<AgentCardManifest>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ConnectionManifest {
    name: String,
    description: String,
    target: ConnectionRefManifest,
    input_modes: Vec<String>,
    output_modes: Vec<String>,
    timeout_seconds: u32,
    max_depth: u32,
    auth: Option<ConnectionAuthManifest>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ConnectionRefManifest {
    internal: Option<InternalConnectionRefManifest>,
    external: Option<ExternalConnectionRefManifest>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct InternalConnectionRefManifest {
    namespace: String,
    agent: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ExternalConnectionRefManifest {
    agent_card_url: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ConnectionAuthManifest {
    kind: String,
    secret_ref: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct AgentCardManifest {
    name: String,
    description: String,
    version: String,
    capabilities: Option<AgentCardCapabilitiesManifest>,
    default_input_modes: Vec<String>,
    default_output_modes: Vec<String>,
    skills: Vec<AgentCardSkillManifest>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct AgentCardCapabilitiesManifest {
    streaming: bool,
    push_notifications: bool,
    extended_agent_card: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct AgentCardSkillManifest {
    id: String,
    name: String,
    description: String,
    tags: Vec<String>,
    examples: Vec<String>,
    input_modes: Vec<String>,
    output_modes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct McpAuthBrokerSpecManifest {
    kind: String,
    url: String,
    cache_ttl_seconds: i32,
    audience: String,
}
