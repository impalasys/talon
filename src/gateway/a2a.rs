// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::sync::Arc;

use axum::{
    extract::{Host, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::gateway::rpc::{manifests, GrpcGatewayHandler};
use crate::gateway::server::Gateway;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentCardJson {
    name: String,
    description: String,
    version: String,
    url: String,
    capabilities: AgentCardCapabilitiesJson,
    default_input_modes: Vec<String>,
    default_output_modes: Vec<String>,
    skills: Vec<AgentCardSkillJson>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentCardCapabilitiesJson {
    streaming: bool,
    push_notifications: bool,
    extended_agent_card: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentCardSkillJson {
    id: String,
    name: String,
    description: String,
    tags: Vec<String>,
    examples: Vec<String>,
    input_modes: Vec<String>,
    output_modes: Vec<String>,
}

pub async fn get_well_known_agent_card(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Host(host): Host,
) -> Response {
    let handler = GrpcGatewayHandler { gateway };
    match handler.find_agent_card_by_hostname(&host).await {
        Ok(Some(card)) => match agent_card_json(&card, scheme_from_headers(&headers, &host), &host)
        {
            Ok(payload) => Json(payload).into_response(),
            Err(response) => response,
        },
        Ok(None) => (StatusCode::NOT_FOUND, "AgentCard not found for host").into_response(),
        Err(status) if status.code() == tonic::Code::InvalidArgument => {
            (StatusCode::BAD_REQUEST, status.message().to_string()).into_response()
        }
        Err(status) => {
            tracing::error!(%status, "Failed to find AgentCard by hostname");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load AgentCard",
            )
                .into_response()
        }
    }
}

fn scheme_from_headers(headers: &HeaderMap, host: &str) -> &'static str {
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
        .unwrap_or_else(|| if is_local_host(host) { "http" } else { "https" })
}

fn host_without_port(host: &str) -> &str {
    let host = host.trim();
    if let Some(stripped) = host.strip_prefix('[') {
        stripped
            .split_once(']')
            .map(|(inside, _rest)| inside)
            .unwrap_or(host)
    } else {
        host.rsplit_once(':')
            .and_then(|(candidate, port)| {
                (!candidate.contains(':') && port.chars().all(|ch| ch.is_ascii_digit()))
                    .then_some(candidate)
            })
            .unwrap_or(host)
    }
}

fn is_local_host(host: &str) -> bool {
    let hostname = host_without_port(host);
    hostname.eq_ignore_ascii_case("localhost") || hostname == "127.0.0.1" || hostname == "::1"
}

fn request_host_port(host: &str) -> Option<&str> {
    let host = host.trim();
    if let Some(stripped) = host.strip_prefix('[') {
        stripped
            .split_once(']')
            .and_then(|(_inside, rest)| rest.strip_prefix(':'))
            .filter(|port| port.chars().all(|ch| ch.is_ascii_digit()))
    } else {
        host.rsplit_once(':').and_then(|(candidate, port)| {
            (!candidate.contains(':') && port.chars().all(|ch| ch.is_ascii_digit())).then_some(port)
        })
    }
}

fn agent_card_json(
    card: &manifests::AgentCard,
    scheme: &str,
    host: &str,
) -> Result<AgentCardJson, Response> {
    let spec = card.spec.as_ref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "stored AgentCard is missing spec",
        )
            .into_response()
    })?;
    let capabilities = spec.capabilities.as_ref();
    let url = if let Some(port) = request_host_port(host) {
        format!("{}://{}:{}", scheme, spec.hostname, port)
    } else {
        format!("{}://{}", scheme, spec.hostname)
    };
    Ok(AgentCardJson {
        name: spec.name.clone(),
        description: spec.description.clone(),
        version: spec.version.clone(),
        url,
        capabilities: AgentCardCapabilitiesJson {
            streaming: capabilities.map(|value| value.streaming).unwrap_or(false),
            push_notifications: capabilities
                .map(|value| value.push_notifications)
                .unwrap_or(false),
            extended_agent_card: capabilities
                .map(|value| value.extended_agent_card)
                .unwrap_or(false),
        },
        default_input_modes: spec.default_input_modes.clone(),
        default_output_modes: spec.default_output_modes.clone(),
        skills: spec
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
