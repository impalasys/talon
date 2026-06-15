// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::control::{keys, ProtoKeyValueStoreExt};
use crate::gateway::auth::{AuthConfig, AuthMode};
use crate::gateway::rpc::{manifests, models};
use crate::gateway::server::Gateway;

use super::a2a_error;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AgentCardJson {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) version: String,
    pub(super) url: String,
    pub(super) protocol_version: String,
    pub(super) preferred_transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) security_schemes: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) security: Vec<HashMap<String, Vec<String>>>,
    pub(super) capabilities: AgentCardCapabilitiesJson,
    pub(super) default_input_modes: Vec<String>,
    pub(super) default_output_modes: Vec<String>,
    pub(super) skills: Vec<AgentCardSkillJson>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AgentCardCapabilitiesJson {
    pub(super) streaming: bool,
    pub(super) push_notifications: bool,
    pub(super) extended_agent_card: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AgentCardSkillJson {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) description: String,
    pub(super) tags: Vec<String>,
    pub(super) examples: Vec<String>,
    pub(super) input_modes: Vec<String>,
    pub(super) output_modes: Vec<String>,
}

#[derive(Clone)]
pub(super) struct AgentCardRoute {
    pub(super) ns: String,
    pub(super) agent: String,
    pub(super) agent_card: manifests::AgentCard,
}

pub(super) fn scheme_from_headers(headers: &HeaderMap) -> &'static str {
    // Deployment must ensure untrusted x-forwarded-* headers are stripped before requests reach
    // the gateway. See docs/operations/deployment-model.md.
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

pub(super) fn external_host_from_headers(headers: &HeaderMap, host: &str) -> String {
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

pub(super) fn agent_card_json(
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
        AuthMode::Password => json!({
            "type": "http",
            "scheme": "basic"
        }),
        AuthMode::Token => json!({
            "type": "http",
            "scheme": "bearer"
        }),
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

pub(super) async fn resolve_agent_card_route(
    gateway: &Arc<Gateway>,
    ns: &str,
    agent_name: &str,
) -> Result<AgentCardRoute, Response> {
    let agent = gateway
        .kv
        .get_msg::<models::Agent>(&keys::agent(ns, agent_name))
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
        .effective_spec
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
        ns: agent.ns,
        agent: agent.name,
        agent_card,
    })
}
