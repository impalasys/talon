// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[allow(unused_imports)]
use crate::talon;

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::gateway::auth::AuthConfig;
use crate::gateway::auth::AuthMode;
use crate::gateway::server::Gateway;
use talon::control::resource_model::TypedResource;
use talon::protocol::manifests;

use super::a2a_error;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCardJson {
    pub name: String,
    pub description: String,
    pub version: String,
    pub url: String,
    pub protocol_version: String,
    pub preferred_transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_schemes: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub security: Vec<HashMap<String, Vec<String>>>,
    pub capabilities: AgentCardCapabilitiesJson,
    pub default_input_modes: Vec<String>,
    pub default_output_modes: Vec<String>,
    pub skills: Vec<AgentCardSkillJson>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCardCapabilitiesJson {
    pub streaming: bool,
    pub push_notifications: bool,
    pub extended_agent_card: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCardSkillJson {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub examples: Vec<String>,
    pub input_modes: Vec<String>,
    pub output_modes: Vec<String>,
}

#[derive(Clone)]
pub struct AgentCardRoute {
    pub ns: String,
    pub agent: String,
    pub agent_card: manifests::AgentCard,
}

pub fn scheme_from_headers(headers: &HeaderMap) -> &'static str {
    // Deployment must ensure untrusted x-forwarded-* headers are stripped before requests reach
    // the gateway. See docs/operate/deployment-model.md.
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .and_then(|value| {
            if value.eq_ignore_ascii_case("http") {
                Some("http")
            } else if value.eq_ignore_ascii_case("https") {
                Some("https")
            } else {
                None
            }
        })
        .unwrap_or("https")
}

pub fn external_host_from_headers(headers: &HeaderMap, host: &str) -> String {
    headers
        .get("x-forwarded-host")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(host)
        .to_string()
}

fn a2a_card_base_url(scheme: &str, host: &str, ns: &str, agent: &str) -> String {
    format!(
        "{}://{}/a2a/{}/{}",
        scheme,
        host.trim(),
        urlencoding::encode(ns),
        urlencoding::encode(agent)
    )
}

pub fn agent_card_json(
    agent_card: &manifests::AgentCard,
    scheme: &str,
    host: &str,
    ns: &str,
    agent: &str,
    auth_config: Option<&AuthConfig>,
) -> Result<AgentCardJson, Response> {
    let capabilities = agent_card.capabilities.as_ref();
    let (security_schemes, security) = agent_card_security(auth_config);
    Ok(AgentCardJson {
        name: agent_card.name.clone(),
        description: agent_card.description.clone(),
        version: agent_card.version.clone(),
        url: a2a_card_base_url(scheme, host, ns, agent),
        protocol_version: "0.3.0".to_string(),
        preferred_transport: "HTTP+JSON".to_string(),
        security_schemes,
        security,
        capabilities: AgentCardCapabilitiesJson {
            streaming: true,
            push_notifications: capabilities
                .map(|value| value.push_notifications)
                .unwrap_or(false),
            extended_agent_card: capabilities
                .map(|value| value.extended_agent_card)
                .unwrap_or(false),
        },
        default_input_modes: agent_card.default_input_modes.clone(),
        default_output_modes: agent_card.default_output_modes.clone(),
        skills: agent_card
            .skills
            .iter()
            .map(|skill| AgentCardSkillJson {
                id: skill.id.clone(),
                name: skill.name.clone(),
                description: skill.description.clone(),
                tags: skill.tags.clone(),
                examples: skill.examples.clone(),
                input_modes: skill.input_modes.clone(),
                output_modes: skill.output_modes.clone(),
            })
            .collect(),
    })
}

fn agent_card_security(
    auth_config: Option<&AuthConfig>,
) -> (
    Option<HashMap<String, Value>>,
    Vec<HashMap<String, Vec<String>>>,
) {
    let Some(auth_config) = auth_config else {
        return (None, Vec::new());
    };
    if auth_config.mode == AuthMode::Open {
        return (None, Vec::new());
    }

    let scheme = match auth_config.mode {
        AuthMode::Open => return (None, Vec::new()),
        AuthMode::Jwt => json!({
            "type": "http",
            "scheme": "bearer",
            "bearerFormat": "JWT"
        }),
    };
    let mut schemes = HashMap::new();
    schemes.insert("talon".to_string(), scheme);
    let mut requirement = HashMap::new();
    requirement.insert("talon".to_string(), Vec::new());
    (Some(schemes), vec![requirement])
}

pub async fn resolve_agent_card_route(
    gateway: &Arc<Gateway>,
    ns: &str,
    agent_name: &str,
) -> Result<AgentCardRoute, Response> {
    let store =
        talon::control::resources::ResourceStore::new(gateway.kv.clone(), gateway.pubsub.clone());
    let agent = store
        .get_agent(ns, agent_name)
        .await
        .map_err(|err| {
            tracing::error!(%err, "Failed to fetch A2A agent");
            a2a_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load A2A agent",
            )
        })?
        .ok_or_else(|| a2a_error(StatusCode::NOT_FOUND, "agent not found"))?;
    let agent_card = agent
        .spec
        .as_ref()
        .and_then(|spec| spec.a2a.as_ref())
        .and_then(|a2a| a2a.agent_card.as_ref())
        .ok_or_else(|| {
            a2a_error(
                StatusCode::NOT_FOUND,
                "agent is not published for external A2A",
            )
        })?
        .clone();
    if agent_card.name.trim().is_empty() {
        return Err(a2a_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "A2A agentCard is missing name",
        ));
    }
    if let Some(capabilities) = agent_card.capabilities.as_ref() {
        if capabilities.push_notifications || capabilities.extended_agent_card {
            return Err(a2a_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "A2A agentCard contains unsupported capabilities",
            ));
        }
    }
    Ok(AgentCardRoute {
        ns: agent.namespace().to_string(),
        agent: agent.name().to_string(),
        agent_card,
    })
}
