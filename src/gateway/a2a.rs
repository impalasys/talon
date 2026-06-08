// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::sync::Arc;

use axum::{
    extract::{Host, State},
    http::StatusCode,
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
    Host(host): Host,
) -> Response {
    let handler = GrpcGatewayHandler { gateway };
    match handler.find_agent_card_by_hostname(&host).await {
        Ok(Some(card)) => match agent_card_json(&card) {
            Ok(payload) => Json(payload).into_response(),
            Err(response) => response,
        },
        Ok(None) => (StatusCode::NOT_FOUND, "AgentCard not found for host").into_response(),
        Err(status) if status.code() == tonic::Code::InvalidArgument => {
            (StatusCode::BAD_REQUEST, status.message().to_string()).into_response()
        }
        Err(status) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to load AgentCard: {}", status.message()),
        )
            .into_response(),
    }
}

fn agent_card_json(card: &manifests::AgentCard) -> Result<AgentCardJson, Response> {
    let spec = card.spec.as_ref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "stored AgentCard is missing spec",
        )
            .into_response()
    })?;
    let capabilities = spec.capabilities.as_ref();
    Ok(AgentCardJson {
        name: spec.name.clone(),
        description: spec.description.clone(),
        version: spec.version.clone(),
        url: format!("https://{}", spec.hostname),
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
