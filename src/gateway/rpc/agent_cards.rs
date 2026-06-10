// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{manifests, models, proto, GrpcGatewayHandler};
use crate::control::ProtoKeyValueStoreExt;
use crate::control::{keys, KeyValueStore};
use prost::Message;

const AGENT_CARD_HOSTNAME_CAS_RETRIES: usize = 8;

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

fn same_card_identity(left: &manifests::AgentCard, right: &manifests::AgentCard) -> bool {
    card_namespace(left) == card_namespace(right) && card_name(left) == card_name(right)
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
    if host.is_empty() {
        return Err(tonic::Status::invalid_argument("Host header is required"));
    }
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
    if let Some(auth) = spec.auth.as_ref() {
        let discovery = auth.discovery.trim();
        if !discovery.is_empty() && discovery != "public" {
            return Err(tonic::Status::invalid_argument(
                "AgentCard spec.auth.discovery must be 'public'; authenticated discovery is not supported yet",
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

async fn claim_agent_card_hostname(
    kv: &(dyn KeyValueStore + Send + Sync),
    hostname: &str,
    card: &manifests::AgentCard,
) -> Result<(), tonic::Status> {
    let key = keys::agent_card_hostname(hostname);
    let value = card.encode_to_vec();

    for _ in 0..AGENT_CARD_HOSTNAME_CAS_RETRIES {
        let current = kv.get(&key).await.map_err(|err| {
            tonic::Status::internal(format!("Failed to fetch AgentCard hostname index: {err}"))
        })?;
        match current {
            Some(current_bytes) => {
                let current_card =
                    manifests::AgentCard::decode(current_bytes.as_slice()).map_err(|err| {
                        tonic::Status::internal(format!(
                            "Failed to decode AgentCard hostname index: {err}"
                        ))
                    })?;
                if !same_card_identity(&current_card, card) {
                    let current_ns = card_namespace(&current_card);
                    let current_name = card_name(&current_card);
                    let current_owner = kv
                        .get_msg::<manifests::AgentCard>(&keys::agent_card(
                            &current_ns,
                            &current_name,
                        ))
                        .await
                        .map_err(|err| {
                            tonic::Status::internal(format!(
                                "Failed to fetch AgentCard hostname owner: {err}"
                            ))
                        })?;
                    if current_owner
                        .as_ref()
                        .and_then(|owner| owner.spec.as_ref())
                        .is_some_and(|spec| spec.hostname == hostname)
                    {
                        return Err(tonic::Status::already_exists(format!(
                            "AgentCard hostname '{}' is already claimed by {}/{}",
                            hostname, current_ns, current_name
                        )));
                    }
                }
                if current_bytes == value {
                    return Ok(());
                }
                if kv
                    .compare_and_swap(&key, Some(current_bytes.as_slice()), &value)
                    .await
                    .map_err(|err| {
                        tonic::Status::internal(format!(
                            "Failed to update AgentCard hostname index: {err}"
                        ))
                    })?
                {
                    return Ok(());
                }
            }
            None => {
                if kv
                    .compare_and_swap(&key, None, &value)
                    .await
                    .map_err(|err| {
                        tonic::Status::internal(format!(
                            "Failed to create AgentCard hostname index: {err}"
                        ))
                    })?
                {
                    return Ok(());
                }
            }
        }
    }

    Err(tonic::Status::aborted(
        "Failed to claim AgentCard hostname after concurrent modifications",
    ))
}

async fn cleanup_agent_card_hostname_index(
    kv: &(dyn KeyValueStore + Send + Sync),
    hostname: &str,
    card: &manifests::AgentCard,
) {
    let key = keys::agent_card_hostname(hostname);
    let expected = card.encode_to_vec();
    if let Err(err) = kv.compare_and_delete(&key, &expected).await {
        tracing::warn!(
            "Failed to clean AgentCard hostname index '{}': {err}",
            hostname
        );
    }
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
        let existing_card = self
            .gateway
            .kv
            .get_msg::<manifests::AgentCard>(&key)
            .await
            .map_err(|err| {
                tonic::Status::internal(format!("Failed to fetch existing AgentCard: {err}"))
            })?;
        claim_agent_card_hostname(self.gateway.kv.as_ref(), &new_hostname, &card).await?;
        self.gateway
            .kv
            .set_msg(&key, &card)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to save AgentCard: {err}")))?;
        if let Some(existing_card) = existing_card {
            if let Some(old_hostname) = existing_card
                .spec
                .as_ref()
                .and_then(|spec| normalize_hostname(&spec.hostname).ok())
            {
                if old_hostname != new_hostname {
                    cleanup_agent_card_hostname_index(
                        self.gateway.kv.as_ref(),
                        &old_hostname,
                        &existing_card,
                    )
                    .await;
                }
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
        let card = self
            .gateway
            .kv
            .get_msg::<manifests::AgentCard>(&key)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to fetch AgentCard: {err}")))?
            .ok_or_else(|| tonic::Status::not_found("AgentCard not found"))?;
        let hostname = card
            .spec
            .as_ref()
            .and_then(|spec| normalize_hostname(&spec.hostname).ok());
        self.gateway
            .kv
            .delete(&key)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to delete AgentCard: {err}")))?;
        if let Some(hostname) = hostname {
            cleanup_agent_card_hostname_index(self.gateway.kv.as_ref(), &hostname, &card).await;
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
        let Some(indexed) = self
            .gateway
            .kv
            .get_msg::<manifests::AgentCard>(&keys::agent_card_hostname(&hostname))
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to fetch AgentCard: {err}")))?
        else {
            return Ok(None);
        };
        let card = self
            .gateway
            .kv
            .get_msg::<manifests::AgentCard>(&keys::agent_card(
                &card_namespace(&indexed),
                &card_name(&indexed),
            ))
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to fetch AgentCard: {err}")))?;
        let Some(card) = card else {
            return Ok(None);
        };
        if card
            .spec
            .as_ref()
            .is_some_and(|spec| spec.hostname == hostname)
        {
            Ok(Some(card))
        } else {
            Ok(None)
        }
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
        assert_eq!(
            request_host_to_hostname("").unwrap_err().message(),
            "Host header is required"
        );
    }
}
