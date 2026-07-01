// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, proto, resources_proto, GrpcGatewayHandler};
use crate::control::ns;
use crate::control::search::{self, Document, ATTR_AGENT, ATTR_CHANNEL, ATTR_SESSION_ID};
use crate::control::{keys, ProtoKeyValueStoreExt};
use crate::gateway::auth::{self, AuthMode, Claims};

impl GrpcGatewayHandler {
    pub async fn handle_search(
        &self,
        req: tonic::Request<proto::SearchRequest>,
    ) -> std::result::Result<tonic::Response<proto::SearchResponse>, tonic::Status> {
        let metadata = req.metadata().clone();
        let mut req = req.into_inner();
        if search::search_namespaces(&req)
            .iter()
            .all(|namespace| namespace.trim().is_empty())
        {
            return Err(tonic::Status::invalid_argument("namespace is required"));
        }
        req.mode = mode(req.mode)? as i32;
        req.sort = sort(req.sort) as i32;
        authorize_search(self, &metadata, &mut req)?;
        let response = self
            .gateway
            .documents
            .search(&req)
            .await
            .map_err(search_error)?;
        Ok(tonic::Response::new(response))
    }

    pub async fn handle_get_search_result(
        &self,
        req: tonic::Request<proto::GetSearchResultRequest>,
    ) -> std::result::Result<tonic::Response<proto::GetSearchResultResponse>, tonic::Status> {
        let metadata = req.metadata().clone();
        let req = req.into_inner();
        if req.ns.trim().is_empty() {
            return Err(tonic::Status::invalid_argument("namespace is required"));
        }
        authorize_search_namespace(self, &metadata, &req.ns)?;
        let document = self
            .gateway
            .documents
            .get_document(&req.ns, &req.document_id)
            .await
            .map_err(search_error)?
            .ok_or_else(|| tonic::Status::not_found("search result not found"))?;
        recheck_document_auth(self, &metadata, &document)?;
        let content = canonical_content(self, &document).await?;
        Ok(tonic::Response::new(proto::GetSearchResultResponse {
            document: Some(document),
            content,
        }))
    }
}

fn authorize_search(
    handler: &GrpcGatewayHandler,
    metadata: &tonic::metadata::MetadataMap,
    query: &mut proto::SearchRequest,
) -> std::result::Result<(), tonic::Status> {
    let namespaces = search::search_namespaces(query);
    let Some(first_namespace) = namespaces.first().copied() else {
        return Ok(());
    };
    let Some(claims) = authorize_search_namespace(handler, metadata, first_namespace)? else {
        return Ok(());
    };
    if !claims.grants.is_empty() {
        for namespace in namespaces {
            ensure_grant_allows_search_namespace(&claims, namespace)?;
        }
    } else if let Some(claim_namespace) = claims.ns.as_ref().filter(|value| !value.is_empty()) {
        for namespace in namespaces {
            if !namespace.is_empty() && !auth::namespace_scope_allows(claim_namespace, namespace) {
                return Err(tonic::Status::permission_denied(format!(
                    "Token scope restricted to namespace: {claim_namespace}"
                )));
            }
        }
    }
    apply_claim_scope(claims, query)
}

fn authorize_search_namespace(
    handler: &GrpcGatewayHandler,
    metadata: &tonic::metadata::MetadataMap,
    namespace: &str,
) -> std::result::Result<Option<Claims>, tonic::Status> {
    let Some(auth_config) = &handler.gateway.auth_config else {
        return Ok(None);
    };
    if auth_config.mode != AuthMode::Jwt {
        auth::check_auth(metadata, auth_config, namespace, None, None)?;
        return Ok(None);
    }
    let Some(claims) = auth::jwt_claims_from_metadata(metadata, auth_config)? else {
        return Ok(None);
    };
    if !claims.grants.is_empty() {
        ensure_grant_allows_search_namespace(&claims, namespace)?;
    } else if let Some(claim_namespace) = claims.ns.as_ref().filter(|value| !value.is_empty()) {
        if !namespace.is_empty() && !auth::namespace_scope_allows(claim_namespace, namespace) {
            return Err(tonic::Status::permission_denied(format!(
                "Token scope restricted to namespace: {claim_namespace}"
            )));
        }
    }
    Ok(Some(claims))
}

fn ensure_grant_allows_search_namespace(
    claims: &Claims,
    namespace: &str,
) -> std::result::Result<(), tonic::Status> {
    if claims.grants.iter().any(|grant| {
        matches!(grant.kind.as_str(), "read" | "readwrite")
            && grant.namespace.as_deref().map_or(true, |allowed| {
                auth::namespace_scope_allows(allowed, namespace)
            })
    }) {
        return Ok(());
    }
    Err(tonic::Status::permission_denied(
        "Token grants do not allow this search namespace",
    ))
}

fn apply_claim_scope(
    claims: Claims,
    query: &mut proto::SearchRequest,
) -> std::result::Result<(), tonic::Status> {
    if !claims.grants.is_empty() {
        return apply_grant_scope(claims.grants, query);
    }
    if let Some(agent) = claims.agent.filter(|value| !value.is_empty()) {
        if let Some(current) = query
            .attributes
            .get(ATTR_AGENT)
            .filter(|value| !value.is_empty())
        {
            if current != &agent {
                return Err(tonic::Status::permission_denied(format!(
                    "Token scope restricted to agent: {agent}"
                )));
            }
        }
        query.attributes.insert(ATTR_AGENT.to_string(), agent);
    }
    if let Some(session) = claims.session.filter(|value| !value.is_empty()) {
        if let Some(current) = query
            .attributes
            .get(ATTR_SESSION_ID)
            .filter(|value| !value.is_empty())
        {
            if current != &session {
                return Err(tonic::Status::permission_denied(format!(
                    "Token scope restricted to session: {session}"
                )));
            }
        }
        query
            .attributes
            .insert(ATTR_SESSION_ID.to_string(), session);
    }
    if let Some(channel) = claims.channel.filter(|value| !value.is_empty()) {
        if let Some(current) = query
            .attributes
            .get(ATTR_CHANNEL)
            .filter(|value| !value.is_empty())
        {
            if current != &channel {
                return Err(tonic::Status::permission_denied(format!(
                    "Token scope restricted to channel: {channel}"
                )));
            }
        }
        query.attributes.insert(ATTR_CHANNEL.to_string(), channel);
    }
    Ok(())
}

fn apply_grant_scope(
    grants: Vec<auth::TalonGrantClaim>,
    query: &mut proto::SearchRequest,
) -> std::result::Result<(), tonic::Status> {
    let namespaces = search::search_namespaces(query)
        .into_iter()
        .map(str::trim)
        .filter(|namespace| !namespace.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut scoped_agent = None;
    let mut scoped_session = None;
    let mut scoped_channel = None;

    for grant in grants {
        if !matches!(grant.kind.as_str(), "read" | "readwrite") {
            continue;
        }
        if !namespaces.iter().any(|namespace| {
            grant.namespace.as_deref().map_or(true, |allowed| {
                auth::namespace_scope_allows(allowed, namespace)
            })
        }) {
            continue;
        }
        let grant_agent = grant.agent.filter(|value| !value.is_empty());
        let grant_session = grant.session.filter(|value| !value.is_empty());
        let grant_channel = grant.channel.filter(|value| !value.is_empty());
        if grant_agent.is_none() && grant_session.is_none() && grant_channel.is_none() {
            return Ok(());
        }
        if scoped_agent.is_none() && scoped_session.is_none() && scoped_channel.is_none() {
            scoped_agent = grant_agent;
            scoped_session = grant_session;
            scoped_channel = grant_channel;
            continue;
        }
        if scoped_agent != grant_agent
            || scoped_session != grant_session
            || scoped_channel != grant_channel
        {
            return Err(tonic::Status::permission_denied(
                "Search with multiple differently scoped grants requires a narrower token",
            ));
        }
    }

    if let Some(agent) = scoped_agent {
        constrain_attribute(query, ATTR_AGENT, agent, "agent")?;
    }
    if let Some(session) = scoped_session {
        constrain_attribute(query, ATTR_SESSION_ID, session, "session")?;
    }
    if let Some(channel) = scoped_channel {
        constrain_attribute(query, ATTR_CHANNEL, channel, "channel")?;
    }
    Ok(())
}

fn constrain_attribute(
    query: &mut proto::SearchRequest,
    key: &str,
    value: String,
    label: &str,
) -> std::result::Result<(), tonic::Status> {
    if let Some(current) = query.attributes.get(key).filter(|value| !value.is_empty()) {
        if current != &value {
            return Err(tonic::Status::permission_denied(format!(
                "Token scope restricted to {label}: {value}"
            )));
        }
    }
    query.attributes.insert(key.to_string(), value);
    Ok(())
}

pub(crate) fn limit(value: i32) -> usize {
    if value <= 0 {
        10
    } else {
        (value as usize).min(100)
    }
}

pub(crate) fn mode(value: i32) -> Result<proto::SearchMode, tonic::Status> {
    match proto::SearchMode::try_from(value).unwrap_or(proto::SearchMode::Keyword) {
        proto::SearchMode::Unspecified | proto::SearchMode::Keyword => {
            Ok(proto::SearchMode::Keyword)
        }
        proto::SearchMode::Semantic => Ok(proto::SearchMode::Semantic),
        proto::SearchMode::Hybrid => Ok(proto::SearchMode::Hybrid),
    }
}

pub(crate) fn sort(value: i32) -> proto::SearchSort {
    match proto::SearchSort::try_from(value).unwrap_or(proto::SearchSort::Relevance) {
        proto::SearchSort::Recency => proto::SearchSort::Recency,
        _ => proto::SearchSort::Relevance,
    }
}

pub(crate) fn search_error(error: anyhow::Error) -> tonic::Status {
    let message = error.to_string();
    if message.contains("search is not enabled for this document store") {
        tonic::Status::unimplemented(message)
    } else if message.contains("document store is not enabled") {
        tonic::Status::unavailable(message)
    } else if message.contains("invalid search page token") {
        tonic::Status::invalid_argument(message)
    } else {
        tonic::Status::internal(message)
    }
}

fn recheck_document_auth(
    handler: &GrpcGatewayHandler,
    metadata: &tonic::metadata::MetadataMap,
    document: &Document,
) -> std::result::Result<(), tonic::Status> {
    let Some(auth_config) = &handler.gateway.auth_config else {
        return Ok(());
    };
    let document_ref = document
        .r#ref
        .as_ref()
        .ok_or_else(|| tonic::Status::internal("search document is missing ref"))?;
    let source = document_ref
        .source
        .as_ref()
        .ok_or_else(|| tonic::Status::internal("search document is missing source"))?;
    match source.kind.as_str() {
        "SessionMessage" => crate::gateway::auth::check_auth(
            metadata,
            auth_config,
            &source.namespace,
            Some(
                document_ref
                    .attributes
                    .get(ATTR_AGENT)
                    .map(String::as_str)
                    .unwrap_or(""),
            ),
            Some(
                document_ref
                    .attributes
                    .get(ATTR_SESSION_ID)
                    .map(String::as_str)
                    .unwrap_or(""),
            ),
        ),
        _ => crate::gateway::auth::check_auth(metadata, auth_config, &source.namespace, None, None),
    }
}

async fn canonical_content(
    handler: &GrpcGatewayHandler,
    document: &Document,
) -> std::result::Result<String, tonic::Status> {
    let document_ref = document
        .r#ref
        .as_ref()
        .ok_or_else(|| tonic::Status::internal("search document is missing ref"))?;
    let source = document_ref
        .source
        .as_ref()
        .ok_or_else(|| tonic::Status::internal("search document is missing source"))?;
    let key = keys::ResourceKey::parse_canonical(&source.key)
        .map_err(|error| tonic::Status::internal(error.to_string()))?;
    match source.kind.as_str() {
        "SessionMessage" => {
            let message = handler
                .gateway
                .kv
                .get_msg::<data_proto::SessionMessage>(&key)
                .await
                .map_err(|error| tonic::Status::internal(error.to_string()))?
                .ok_or_else(|| tonic::Status::not_found("session message not found"))?;
            serde_json::to_string(&message)
                .map_err(|error| tonic::Status::internal(error.to_string()))
        }
        _ => {
            let store = crate::control::resources::ResourceStore::new(
                handler.gateway.kv.clone(),
                handler.gateway.pubsub.clone(),
            );
            let resource = store
                .get(&key.namespace, &key.kind, &key.name)
                .await
                .map_err(|error| tonic::Status::internal(error.to_string()))?
                .ok_or_else(|| tonic::Status::not_found("resource not found"))?;
            if source.kind == "Knowledge" && document_ref.document_kind == "content" {
                let Some(resources_proto::resource_spec::Kind::Knowledge(spec)) =
                    resource.spec.and_then(|spec| spec.kind)
                else {
                    return Err(tonic::Status::not_found("knowledge resource not found"));
                };
                Ok(spec.content)
            } else {
                serde_json::to_string(&resource)
                    .map_err(|error| tonic::Status::internal(error.to_string()))
            }
        }
    }
}

pub(crate) fn knowledge_namespaces(namespace: &str) -> Vec<String> {
    ns::ancestry(namespace)
}
