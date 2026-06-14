// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{manifests, models, proto, GrpcGatewayHandler};
use crate::control::keys;
use crate::control::ProtoKeyValueStoreExt;
use prost::Message;

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

fn decode_agent_card(bytes: &[u8], context: &str) -> Result<manifests::AgentCard, tonic::Status> {
    manifests::AgentCard::decode(bytes)
        .map_err(|err| tonic::Status::internal(format!("Failed to decode {context}: {err}")))
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
    if let Some(capabilities) = spec.capabilities.as_ref() {
        if capabilities.push_notifications {
            return Err(tonic::Status::invalid_argument(
                "AgentCard spec.capabilities.pushNotifications is not supported yet",
            ));
        }
        if capabilities.extended_agent_card {
            return Err(tonic::Status::invalid_argument(
                "AgentCard spec.capabilities.extendedAgentCard is not supported yet",
            ));
        }
    }
    if let Some(auth) = spec.auth.as_ref() {
        let discovery = auth.discovery.trim();
        if !discovery.is_empty() && discovery != "public" {
            return Err(tonic::Status::invalid_argument(
                "AgentCard spec.auth.discovery must be 'public'; authenticated discovery is not supported yet",
            ));
        }
        let operations = auth.operations.trim();
        if !operations.is_empty() && operations != "public" {
            return Err(tonic::Status::invalid_argument(
                "AgentCard spec.auth.operations must be 'public'; authenticated A2A operations are not supported yet",
            ));
        }
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
        let key = keys::agent_card(&req.ns, &name);
        let existing_bytes = self.gateway.kv.get(&key).await.map_err(|err| {
            tonic::Status::internal(format!("Failed to fetch existing AgentCard: {err}"))
        })?;
        let card_bytes = card.encode_to_vec();
        match self
            .gateway
            .kv
            .compare_and_swap(&key, existing_bytes.as_deref(), &card_bytes)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to save AgentCard: {err}")))?
        {
            true => {}
            false => {
                return Err(tonic::Status::aborted(
                    "AgentCard changed while saving; retry the request",
                ));
            }
        }

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

        let futures = keys.iter().map(|key| async {
            self.gateway
                .kv
                .get_msg::<manifests::AgentCard>(key)
                .await
                .map_err(|err| tonic::Status::internal(format!("Failed to fetch AgentCard: {err}")))
        });
        let cards = futures::future::try_join_all(futures)
            .await?
            .into_iter()
            .flatten()
            .collect();
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
        let card_bytes = self
            .gateway
            .kv
            .get(&key)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to fetch AgentCard: {err}")))?
            .ok_or_else(|| tonic::Status::not_found("AgentCard not found"))?;
        let card = decode_agent_card(&card_bytes, "AgentCard")?;
        drop(card);
        let deleted = self
            .gateway
            .kv
            .compare_and_delete(&key, &card_bytes)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to delete AgentCard: {err}")))?;
        if !deleted {
            return Err(tonic::Status::aborted(
                "AgentCard changed while deleting; retry the request",
            ));
        }
        Ok(tonic::Response::new(proto::DeleteAgentCardResponse {
            success: true,
        }))
    }
}
