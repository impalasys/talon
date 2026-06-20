// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, proto, resources_proto, GrpcGatewayHandler};
use crate::control::ns;
use crate::control::search::{self, Document, SearchMode, SearchQuery, SearchSort};
use crate::control::{keys, ProtoKeyValueStoreExt};
use crate::gateway::auth::{self, AuthMode, Claims};

impl GrpcGatewayHandler {
    pub async fn handle_search(
        &self,
        req: tonic::Request<proto::SearchRequest>,
    ) -> std::result::Result<tonic::Response<proto::SearchResponse>, tonic::Status> {
        let metadata = req.metadata().clone();
        let req = req.into_inner();
        let mut query = SearchQuery {
            query: req.query,
            namespaces: vec![req.ns],
            resource_kinds: req.resource_kinds,
            agent: req.agent,
            session_id: req.session_id,
            channel: req.channel,
            role: req.role,
            part_type: req.part_type,
            labels: req.labels,
            start_time: req.start_time,
            end_time: req.end_time,
            limit: limit(req.limit),
            page_token: req.page_token,
            mode: mode(req.mode)?,
            sort: sort(req.sort),
        };
        authorize_search(self, &metadata, &mut query)?;
        let response = self
            .gateway
            .documents
            .search(&query)
            .await
            .map_err(search_error)?;
        Ok(tonic::Response::new(search_response_proto(response)))
    }

    pub async fn handle_get_search_result(
        &self,
        req: tonic::Request<proto::GetSearchResultRequest>,
    ) -> std::result::Result<tonic::Response<proto::GetSearchResultResponse>, tonic::Status> {
        let metadata = req.metadata().clone();
        let req = req.into_inner();
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
            document: Some(document_proto(document)),
            content,
        }))
    }
}

fn authorize_search(
    handler: &GrpcGatewayHandler,
    metadata: &tonic::metadata::MetadataMap,
    query: &mut SearchQuery,
) -> std::result::Result<(), tonic::Status> {
    let namespace = query
        .namespaces
        .first()
        .map(String::as_str)
        .unwrap_or_default();
    let Some(claims) = authorize_search_namespace(handler, metadata, namespace)? else {
        return Ok(());
    };
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
    if let Some(claim_namespace) = claims.ns.as_ref().filter(|value| !value.is_empty()) {
        if !namespace.is_empty() && namespace != claim_namespace {
            return Err(tonic::Status::permission_denied(format!(
                "Token scope restricted to namespace: {claim_namespace}"
            )));
        }
    }
    Ok(Some(claims))
}

fn apply_claim_scope(
    claims: Claims,
    query: &mut SearchQuery,
) -> std::result::Result<(), tonic::Status> {
    if let Some(agent) = claims.agent.filter(|value| !value.is_empty()) {
        if !query.agent.is_empty() && query.agent != agent {
            return Err(tonic::Status::permission_denied(format!(
                "Token scope restricted to agent: {agent}"
            )));
        }
        query.agent = agent;
    }
    if let Some(session) = claims.session.filter(|value| !value.is_empty()) {
        if !query.session_id.is_empty() && query.session_id != session {
            return Err(tonic::Status::permission_denied(format!(
                "Token scope restricted to session: {session}"
            )));
        }
        query.session_id = session;
    }
    if let Some(channel) = claims.channel.filter(|value| !value.is_empty()) {
        if !query.channel.is_empty() && query.channel != channel {
            return Err(tonic::Status::permission_denied(format!(
                "Token scope restricted to channel: {channel}"
            )));
        }
        query.channel = channel;
    }
    Ok(())
}

pub(crate) fn search_response_proto(response: search::SearchResponse) -> proto::SearchResponse {
    proto::SearchResponse {
        results: response
            .results
            .into_iter()
            .map(|result| proto::SearchResult {
                document: Some(document_proto(result.document)),
                score: result.score,
            })
            .collect(),
        next_page_token: response.next_page_token,
    }
}

pub(crate) fn document_proto(document: Document) -> proto::Document {
    proto::Document {
        id: document.id,
        namespace: document.namespace,
        resource_kind: document.resource_kind,
        resource_key: document.resource_key,
        document_kind: document.document_kind,
        parent_kind: document.parent_kind,
        parent_key: document.parent_key,
        agent: document.agent,
        session_id: document.session_id,
        channel: document.channel,
        message_id: document.message_id,
        run_id: document.run_id,
        part_id: document.part_id,
        part_type: document.part_type,
        role: document.role,
        title: document.title,
        snippet: document.snippet,
        labels: document.labels,
        metadata_json: document.metadata_json,
        acl_scope_json: document.acl_scope_json,
        created_at: document.created_at,
        updated_at: document.updated_at,
        indexed_at: document.indexed_at,
        source_generation: document.source_generation,
        embedding_ref: document.embedding_ref,
    }
}

pub(crate) fn limit(value: i32) -> usize {
    if value <= 0 {
        10
    } else {
        (value as usize).min(100)
    }
}

pub(crate) fn mode(value: i32) -> Result<SearchMode, tonic::Status> {
    match proto::SearchMode::try_from(value).unwrap_or(proto::SearchMode::Keyword) {
        proto::SearchMode::Unspecified | proto::SearchMode::Keyword => Ok(SearchMode::Keyword),
        proto::SearchMode::Semantic => Ok(SearchMode::Semantic),
        proto::SearchMode::Hybrid => Ok(SearchMode::Hybrid),
    }
}

pub(crate) fn sort(value: i32) -> SearchSort {
    match proto::SearchSort::try_from(value).unwrap_or(proto::SearchSort::Relevance) {
        proto::SearchSort::Recency => SearchSort::Recency,
        _ => SearchSort::Relevance,
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
    match document.resource_kind.as_str() {
        "SessionMessage" => crate::gateway::auth::check_auth(
            metadata,
            auth_config,
            &document.namespace,
            Some(&document.agent),
            Some(&document.session_id),
        ),
        _ => {
            crate::gateway::auth::check_auth(metadata, auth_config, &document.namespace, None, None)
        }
    }
}

async fn canonical_content(
    handler: &GrpcGatewayHandler,
    document: &Document,
) -> std::result::Result<String, tonic::Status> {
    let key = keys::ResourceKey::parse_canonical(&document.resource_key)
        .map_err(|error| tonic::Status::internal(error.to_string()))?;
    match document.resource_kind.as_str() {
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
            if document.resource_kind == "Knowledge" && document.document_kind == "content" {
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
