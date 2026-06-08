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

fn normalize_hostname(hostname: &str) -> Result<String, tonic::Status> {
    let hostname = hostname.trim().trim_end_matches('.').to_ascii_lowercase();
    if hostname.is_empty() {
        return Err(tonic::Status::invalid_argument(
            "AgentCard spec.hostname is required",
        ));
    }
    if hostname.contains("://")
        || hostname.contains('/')
        || hostname.contains('\\')
        || hostname.contains(':')
        || hostname.chars().any(char::is_whitespace)
    {
        return Err(tonic::Status::invalid_argument(
            "AgentCard spec.hostname must be a hostname without scheme, port, path, or whitespace",
        ));
    }
    if hostname.len() > 253 {
        return Err(tonic::Status::invalid_argument(
            "AgentCard spec.hostname cannot exceed 253 characters",
        ));
    }
    for label in hostname.split('.') {
        if label.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "AgentCard spec.hostname contains an empty label",
            ));
        }
        if label.len() > 63 {
            return Err(tonic::Status::invalid_argument(
                "AgentCard spec.hostname label cannot exceed 63 characters",
            ));
        }
        let valid_chars = label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
        if !valid_chars || label.starts_with('-') || label.ends_with('-') {
            return Err(tonic::Status::invalid_argument(
                "AgentCard spec.hostname must contain valid DNS labels",
            ));
        }
    }
    Ok(hostname)
}

fn request_host_to_hostname(host: &str) -> Result<String, tonic::Status> {
    let host = host.trim();
    let host = if let Some(stripped) = host.strip_prefix('[') {
        let Some((inside, _rest)) = stripped.split_once(']') else {
            return Err(tonic::Status::invalid_argument("invalid Host header"));
        };
        inside
    } else {
        host.rsplit_once(':')
            .and_then(|(candidate, port)| {
                (!candidate.contains(':') && port.chars().all(|ch| ch.is_ascii_digit()))
                    .then_some(candidate)
            })
            .unwrap_or(host)
    };
    normalize_hostname(host)
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
    let hostname = normalize_hostname(&spec.hostname)?;

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

    if let Some(existing) = handler.find_agent_card_by_hostname(&hostname).await? {
        let existing_name = card_name(&existing);
        let existing_ns = card_namespace(&existing);
        if existing_ns != ns || existing_name != name {
            return Err(tonic::Status::already_exists(format!(
                "AgentCard hostname '{}' is already claimed by {}/{}",
                hostname, existing_ns, existing_name
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
        if let Some(spec) = card.spec.as_mut() {
            spec.hostname = normalize_hostname(&spec.hostname)?;
        }

        validate_agent_card(self, &req.ns, &card).await?;
        let name = card_name(&card);
        let new_hostname = card_spec(&card)?.hostname.clone();
        let key = keys::agent_card(&req.ns, &name);
        let old_hostname = self
            .gateway
            .kv
            .get_msg::<manifests::AgentCard>(&key)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to fetch AgentCard: {err}")))?
            .and_then(|old| old.spec.map(|spec| spec.hostname));
        self.gateway
            .kv
            .set_msg(&key, &card)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to save AgentCard: {err}")))?;
        self.gateway
            .kv
            .set_msg(&keys::agent_card_hostname(&new_hostname), &card)
            .await
            .map_err(|err| {
                tonic::Status::internal(format!("Failed to index AgentCard hostname: {err}"))
            })?;
        if let Some(old_hostname) = old_hostname {
            let old_hostname = normalize_hostname(&old_hostname)?;
            if old_hostname != new_hostname {
                self.gateway
                    .kv
                    .delete(&keys::agent_card_hostname(&old_hostname))
                    .await
                    .map_err(|err| {
                        tonic::Status::internal(format!(
                            "Failed to delete old AgentCard hostname index: {err}"
                        ))
                    })?;
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
        let old_hostname = self
            .gateway
            .kv
            .get_msg::<manifests::AgentCard>(&key)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to fetch AgentCard: {err}")))?
            .and_then(|card| card.spec.map(|spec| spec.hostname));
        self.gateway
            .kv
            .delete(&key)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to delete AgentCard: {err}")))?;
        if let Some(old_hostname) = old_hostname {
            let old_hostname = normalize_hostname(&old_hostname)?;
            self.gateway
                .kv
                .delete(&keys::agent_card_hostname(&old_hostname))
                .await
                .map_err(|err| {
                    tonic::Status::internal(format!(
                        "Failed to delete AgentCard hostname index: {err}"
                    ))
                })?;
        }
        Ok(tonic::Response::new(proto::DeleteAgentCardResponse {
            success: true,
        }))
    }

    pub async fn find_agent_card_by_hostname(
        &self,
        host: &str,
    ) -> Result<Option<manifests::AgentCard>, tonic::Status> {
        let hostname = request_host_to_hostname(host)?;
        self.gateway
            .kv
            .get_msg::<manifests::AgentCard>(&keys::agent_card_hostname(&hostname))
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to fetch AgentCard: {err}")))
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_hostname, request_host_to_hostname};

    #[test]
    fn normalize_hostname_rejects_scheme_port_path_and_bad_labels() {
        assert_eq!(
            normalize_hostname("Support.Example.COM.").unwrap(),
            "support.example.com"
        );
        assert!(normalize_hostname("https://support.example.com").is_err());
        assert!(normalize_hostname("support.example.com/path").is_err());
        assert!(normalize_hostname("support.example.com:443").is_err());
        assert!(normalize_hostname("-support.example.com").is_err());
        assert!(normalize_hostname("support..example.com").is_err());
    }

    #[test]
    fn request_host_to_hostname_accepts_request_port() {
        assert_eq!(
            request_host_to_hostname("support.example.com:443").unwrap(),
            "support.example.com"
        );
    }
}
