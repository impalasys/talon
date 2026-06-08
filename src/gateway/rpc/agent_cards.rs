// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{manifests, models, proto, GrpcGatewayHandler};
use crate::control::keys;
use crate::control::ProtoKeyValueStoreExt;

fn card_name(card: &manifests::AgentCard) -> String {
    card.metadata
        .as_ref()
        .map(|metadata| metadata.name.clone())
        .unwrap_or_default()
}

fn card_namespace(card: &manifests::AgentCard) -> String {
    card.metadata
        .as_ref()
        .map(|metadata| metadata.namespace.clone())
        .unwrap_or_default()
}

fn card_spec(card: &manifests::AgentCard) -> Result<&manifests::AgentCardSpec, tonic::Status> {
    card.spec
        .as_ref()
        .ok_or_else(|| tonic::Status::invalid_argument("AgentCard missing spec"))
}

async fn validate_agent_card(
    handler: &GrpcGatewayHandler,
    ns: &str,
    card: &manifests::AgentCard,
) -> Result<(), tonic::Status> {
    let name = card_name(card);
    if name.trim().is_empty() {
        return Err(tonic::Status::invalid_argument(
            "AgentCard metadata.name is required",
        ));
    }

    let card_ns = card_namespace(card);
    if card_ns != ns {
        return Err(tonic::Status::invalid_argument(
            "AgentCard metadata.namespace must match request namespace",
        ));
    }

    let spec = card_spec(card)?;
    if spec.agent_ref.trim().is_empty() {
        return Err(tonic::Status::invalid_argument(
            "AgentCard spec.agentRef is required",
        ));
    }
    if spec.hostname.trim().is_empty() {
        return Err(tonic::Status::invalid_argument(
            "AgentCard spec.hostname is required",
        ));
    }

    handler
        .gateway
        .kv
        .get_msg::<models::Agent>(&keys::agent(ns, &spec.agent_ref))
        .await
        .map_err(|err| tonic::Status::internal(format!("Failed to verify agent: {err}")))?
        .ok_or_else(|| {
            tonic::Status::failed_precondition(format!(
                "Agent '{}' not found in namespace '{}'",
                spec.agent_ref, ns
            ))
        })?;

    if let Some(existing) = handler.find_agent_card_by_hostname(&spec.hostname).await? {
        let existing_name = card_name(&existing);
        let existing_ns = card_namespace(&existing);
        if existing_ns != ns || existing_name != name {
            return Err(tonic::Status::already_exists(format!(
                "AgentCard hostname '{}' is already claimed by {}/{}",
                spec.hostname, existing_ns, existing_name
            )));
        }
    }

    Ok(())
}

impl GrpcGatewayHandler {
    pub async fn handle_create_agent_card(
        &self,
        req: tonic::Request<proto::CreateAgentCardRequest>,
    ) -> Result<tonic::Response<proto::AgentCardResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let mut card = req
            .card
            .ok_or_else(|| tonic::Status::invalid_argument("missing AgentCard"))?;

        if card.api_version.is_empty() {
            card.api_version = "talon.impalasys.com/v1".to_string();
        }
        if card.kind.is_empty() {
            card.kind = "AgentCard".to_string();
        }
        if let Some(metadata) = card.metadata.as_mut() {
            if metadata.namespace.is_empty() {
                metadata.namespace = req.ns.clone();
            }
        }

        validate_agent_card(self, &req.ns, &card).await?;
        let name = card_name(&card);
        self.gateway
            .kv
            .set_msg(&keys::agent_card(&req.ns, &name), &card)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to save AgentCard: {err}")))?;

        Ok(tonic::Response::new(proto::AgentCardResponse {
            card: Some(card),
        }))
    }

    pub async fn handle_get_agent_card(
        &self,
        req: tonic::Request<proto::GetAgentCardRequest>,
    ) -> Result<tonic::Response<proto::AgentCardResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let card = self
            .gateway
            .kv
            .get_msg::<manifests::AgentCard>(&keys::agent_card(&req.ns, &req.name))
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to fetch AgentCard: {err}")))?
            .ok_or_else(|| tonic::Status::not_found("AgentCard not found"))?;

        Ok(tonic::Response::new(proto::AgentCardResponse {
            card: Some(card),
        }))
    }

    pub async fn handle_list_agent_cards(
        &self,
        req: tonic::Request<proto::ListAgentCardsRequest>,
    ) -> Result<tonic::Response<proto::ListAgentCardsResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let keys = self
            .gateway
            .kv
            .list_keys(&keys::agent_card_prefix(&req.ns))
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to list AgentCards: {err}")))?;

        let mut cards = Vec::new();
        for key in keys {
            if let Some(card) = self
                .gateway
                .kv
                .get_msg::<manifests::AgentCard>(&key)
                .await
                .map_err(|err| {
                    tonic::Status::internal(format!("Failed to fetch AgentCard: {err}"))
                })?
            {
                cards.push(card);
            }
        }
        Ok(tonic::Response::new(proto::ListAgentCardsResponse {
            cards,
        }))
    }

    pub async fn handle_delete_agent_card(
        &self,
        req: tonic::Request<proto::DeleteAgentCardRequest>,
    ) -> Result<tonic::Response<proto::DeleteAgentCardResponse>, tonic::Status> {
        crate::require_auth!(self, req, &req.get_ref().ns);
        let req = req.into_inner();
        let key = keys::agent_card(&req.ns, &req.name);
        let exists = self
            .gateway
            .kv
            .get(&key)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to fetch AgentCard: {err}")))?
            .is_some();
        if !exists {
            return Err(tonic::Status::not_found("AgentCard not found"));
        }
        self.gateway
            .kv
            .delete(&key)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to delete AgentCard: {err}")))?;
        Ok(tonic::Response::new(proto::DeleteAgentCardResponse {
            success: true,
        }))
    }

    pub async fn find_agent_card_by_hostname(
        &self,
        hostname: &str,
    ) -> Result<Option<manifests::AgentCard>, tonic::Status> {
        let hostname = hostname
            .trim()
            .trim_end_matches('.')
            .split_once(':')
            .map(|(host, _)| host)
            .unwrap_or_else(|| hostname.trim().trim_end_matches('.'));
        let namespace_keys = self
            .gateway
            .kv
            .list_keys(&keys::namespace_metadata_prefix())
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to list namespaces: {err}")))?;

        for namespace_key in namespace_keys {
            let ns = namespace_key.name;
            let card_keys = self
                .gateway
                .kv
                .list_keys(&keys::agent_card_prefix(&ns))
                .await
                .map_err(|err| {
                    tonic::Status::internal(format!("Failed to list AgentCards: {err}"))
                })?;
            for card_key in card_keys {
                let Some(card) = self
                    .gateway
                    .kv
                    .get_msg::<manifests::AgentCard>(&card_key)
                    .await
                    .map_err(|err| {
                        tonic::Status::internal(format!("Failed to fetch AgentCard: {err}"))
                    })?
                else {
                    continue;
                };
                let Some(spec) = card.spec.as_ref() else {
                    continue;
                };
                if spec.hostname.trim().trim_end_matches('.') == hostname {
                    return Ok(Some(card));
                }
            }
        }

        Ok(None)
    }
}
