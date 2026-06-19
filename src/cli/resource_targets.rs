fn agent_lookup_target(name: &str, namespace: Option<&String>) -> (String, String) {
    let mut parts = name.splitn(2, '/');
    let ns_part = parts.next().unwrap_or("default");
    let agent_name = parts.next().unwrap_or(ns_part);
    let (mut final_ns, final_name) = if agent_name == ns_part {
        ("default".to_string(), ns_part.to_string())
    } else {
        (ns_part.to_string(), agent_name.to_string())
    };
    if let Some(n) = namespace {
        final_ns = n.clone();
    }
    (final_ns, final_name)
}

fn rest_get_path(
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<(String, &'static str)> {
    match kind.to_lowercase().as_str() {
        "agenttemplate" | "templates" | "template" => {
            let ns = namespace
                .cloned()
                .unwrap_or_else(|| crate::control::ns::TALON_SYSTEM.to_string());
            Ok((
                format!(
                    "/v1/ns/{}/resources/Template/{}",
                    urlencoding::encode(&ns),
                    urlencoding::encode(name)
                ),
                "resource",
            ))
        }
        "mcpserver" | "mcpservers" | "mcp" => Ok((
            format!("/v1/mcp-servers/{}", urlencoding::encode(name)),
            "server",
        )),
        "agent" | "agents" => {
            let (ns, agent_name) = agent_lookup_target(name, namespace);
            Ok((
                format!(
                    "/v1/ns/{}/agents/{}",
                    urlencoding::encode(&ns),
                    urlencoding::encode(&agent_name)
                ),
                "agent",
            ))
        }
        "mcpserverbinding" | "mcpbindings" | "mcpbinding" => {
            let ns = namespace
                .as_ref()
                .context("namespace is required for McpServerBinding get")?;
            Ok((
                format!(
                    "/v1/namespaces/{}/mcp-bindings/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "binding",
            ))
        }
        "namespace" | "namespaces" => Ok((
            format!("/v1/namespaces/{}", urlencoding::encode(name)),
            "namespace",
        )),
        "knowledge" | "knowledgeartifact" | "knowledgeartifacts" => {
            let ns = namespace
                .as_ref()
                .context("Knowledge get requires --namespace")?;
            Ok((
                format!(
                    "/v1/namespaces/{}/knowledge/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "knowledge",
            ))
        }
        "schedule" | "schedules" => {
            let ns = namespace
                .as_ref()
                .context("Schedule get requires --namespace")?;
            Ok((
                format!(
                    "/v1/ns/{}/schedules/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "schedule",
            ))
        }
        "channel" | "channels" => {
            let ns = namespace
                .as_ref()
                .context("Channel get requires --namespace")?;
            Ok((
                format!(
                    "/v1/ns/{}/channels/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "channel",
            ))
        }
        "channelsubscription"
        | "channelsubscriptions"
        | "channel-subscription"
        | "channel-subscriptions" => {
            let ns = namespace
                .as_ref()
                .context("ChannelSubscription get requires --namespace")?;
            let (channel, subscription) = name
                .split_once('/')
                .context("ChannelSubscription name must be '<channel>/<subscription>'")?;
            Ok((
                format!(
                    "/v1/ns/{}/channels/{}/subscriptions/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(channel),
                    urlencoding::encode(subscription)
                ),
                "subscription",
            ))
        }
        "workflow" | "workflows" => {
            let ns = namespace
                .as_ref()
                .context("Workflow get requires --namespace")?;
            Ok((
                format!(
                    "/v1/ns/{}/workflows/{}",
                    urlencoding::encode(ns),
                    urlencoding::encode(name)
                ),
                "workflow",
            ))
        }
        other => anyhow::bail!("Unsupported resource kind '{}' for REST mode", other),
    }
}

fn rest_delete_path(kind: &str, name: &str, namespace: Option<&String>) -> Result<String> {
    if matches!(kind.to_lowercase().as_str(), "namespace" | "namespaces") {
        return Ok(format!("/v1/namespaces/{}", urlencoding::encode(name)));
    }
    let (ns, resource_kind, resource_name) = resource_lookup_target(kind, name, namespace)?;
    Ok(format!(
        "/v1/ns/{}/resources/{}/{}",
        urlencoding::encode(&ns),
        urlencoding::encode(&resource_kind),
        urlencoding::encode(&resource_name)
    ))
}
