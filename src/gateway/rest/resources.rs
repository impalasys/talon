// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::gateway::rpc::{proto, resources_proto, GrpcGatewayHandler};
use crate::gateway::server::Gateway;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tonic::metadata::MetadataValue;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateNamespaceBody {
    name: String,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    labels: HashMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateResourceBody {
    ns: String,
    manifest: Value,
}

#[derive(Deserialize)]
struct ListResourcesQuery {
    kind: Option<String>,
}

#[derive(Deserialize)]
struct ListNamespacesQuery {
    parent: Option<String>,
}

pub(crate) fn router() -> Router<Arc<Gateway>> {
    Router::new()
        .route(
            "/v1/ns/:ns/resources",
            get(list_resources).post(create_resource),
        )
        .route("/v1/ns/:ns/resources/:kind/*name", get(get_resource))
        .route("/v1/namespaces", get(list_namespaces))
        .route(
            "/v1/namespaces/:name",
            get(get_namespace).post(create_namespace),
        )
        .route("/v1/mcp-servers/:name", get(get_mcp_server))
        .route("/v1/ns/:ns/agents/:name", get(get_agent))
        .route(
            "/v1/namespaces/:ns/mcp-bindings/:name",
            get(get_mcp_binding),
        )
        .route("/v1/namespaces/:ns/knowledge/*name", get(get_knowledge))
}

fn handler(gateway: Arc<Gateway>) -> GrpcGatewayHandler {
    GrpcGatewayHandler { gateway }
}

fn tonic_request<T>(headers: &HeaderMap, message: T) -> Result<tonic::Request<T>, Response> {
    let mut request = tonic::Request::new(message);
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let value = metadata_value(value)?;
        request.metadata_mut().insert("authorization", value);
    }
    Ok(request)
}

fn metadata_value(value: &HeaderValue) -> Result<MetadataValue<tonic::metadata::Ascii>, Response> {
    let value = value
        .to_str()
        .map_err(|_| json_error(StatusCode::BAD_REQUEST, "invalid authorization header"))?;
    MetadataValue::try_from(value)
        .map_err(|_| json_error(StatusCode::BAD_REQUEST, "invalid authorization header"))
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

fn status_response(status: tonic::Status) -> Response {
    let status_code = match status.code() {
        tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
        tonic::Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
        tonic::Code::NotFound => StatusCode::NOT_FOUND,
        tonic::Code::FailedPrecondition => StatusCode::PRECONDITION_FAILED,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    json_error(status_code, status.message())
}

fn namespace_json(namespace: proto::NamespaceResponse) -> Value {
    json!({
        "name": namespace.name,
        "parent": namespace.parent,
        "isDeleted": namespace.is_deleted,
        "deletedAt": namespace.deleted_at,
        "labels": namespace.labels,
    })
}

fn resource_json(resource: resources_proto::Resource) -> Value {
    serde_json::to_value(resource).unwrap_or_else(|_| json!({}))
}

fn typed_resource_json(resource: resources_proto::Resource, kind: &str) -> Result<Value, Response> {
    let metadata = resource.metadata.clone();
    let spec = resource.spec.and_then(|spec| spec.kind);
    let status = resource.status.and_then(|status| status.kind);
    match kind {
        "Agent" => Ok(json!({
            "metadata": metadata,
            "spec": match spec {
                Some(resources_proto::resource_spec::Kind::Agent(spec)) => Some(spec),
                _ => None,
            },
            "status": match status {
                Some(resources_proto::resource_status::Kind::Agent(status)) => Some(status),
                _ => None,
            },
        })),
        "Knowledge" => Ok(json!({
            "metadata": metadata,
            "spec": match spec {
                Some(resources_proto::resource_spec::Kind::Knowledge(spec)) => Some(spec),
                _ => None,
            },
            "status": match status {
                Some(resources_proto::resource_status::Kind::Knowledge(status)) => Some(status),
                _ => None,
            },
        })),
        "McpServer" => Ok(json!({
            "metadata": metadata,
            "spec": match spec {
                Some(resources_proto::resource_spec::Kind::McpServer(spec)) => Some(spec),
                _ => None,
            },
            "status": match status {
                Some(resources_proto::resource_status::Kind::McpServer(status)) => Some(status),
                _ => None,
            },
        })),
        "McpServerBinding" => Ok(json!({
            "metadata": metadata,
            "spec": match spec {
                Some(resources_proto::resource_spec::Kind::McpServerBinding(spec)) => Some(spec),
                _ => None,
            },
            "status": match status {
                Some(resources_proto::resource_status::Kind::McpServerBinding(status)) => Some(status),
                _ => None,
            },
        })),
        _ => Err(json_error(
            StatusCode::NOT_FOUND,
            "unsupported typed resource",
        )),
    }
}

fn resource_manifest_from_json(
    value: Value,
) -> Result<resources_proto::ResourceManifest, Response> {
    let api_version = value
        .get("apiVersion")
        .or_else(|| value.get("api_version"))
        .and_then(Value::as_str)
        .unwrap_or("talon.impalasys.com/v1")
        .to_string();
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "resource manifest kind is required",
            )
        })?
        .to_string();
    let metadata = value
        .get("metadata")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| json_error(StatusCode::BAD_REQUEST, "invalid resource metadata"))?;
    let spec_value = value.get("spec").cloned().unwrap_or_else(|| json!({}));
    let spec = Some(resources_proto::ResourceSpec {
        kind: resource_spec_kind_from_json(&kind, spec_value)?,
    });
    Ok(resources_proto::ResourceManifest {
        api_version,
        kind,
        metadata,
        spec,
    })
}

fn resource_spec_kind_from_json(
    kind: &str,
    spec_value: Value,
) -> Result<Option<resources_proto::resource_spec::Kind>, Response> {
    use resources_proto::resource_spec::Kind;

    fn field(spec_value: &Value, field: &str) -> Value {
        spec_value
            .get(field)
            .cloned()
            .unwrap_or_else(|| spec_value.clone())
    }

    macro_rules! decode_kind {
        ($variant:ident, $field:literal) => {
            Some(Kind::$variant(
                serde_json::from_value(field(&spec_value, $field))
                    .map_err(|_| json_error(StatusCode::BAD_REQUEST, "invalid resource spec"))?,
            ))
        };
    }

    let spec = match kind {
        "Agent" => decode_kind!(Agent, "agent"),
        "Workflow" => decode_kind!(Workflow, "workflow"),
        "Schedule" => decode_kind!(Schedule, "schedule"),
        "Channel" => decode_kind!(Channel, "channel"),
        "ChannelSubscription" => decode_kind!(ChannelSubscription, "channelSubscription"),
        "McpServer" => decode_kind!(McpServer, "mcpServer"),
        "McpServerBinding" => decode_kind!(McpServerBinding, "mcpServerBinding"),
        "Knowledge" => decode_kind!(Knowledge, "knowledge"),
        "Namespace" => decode_kind!(Namespace, "namespace"),
        "Session" => decode_kind!(Session, "session"),
        "Skill" => decode_kind!(Skill, "skill"),
        "Template" => decode_kind!(Template, "template"),
        "Deployment" => decode_kind!(Deployment, "deployment"),
        "DeploymentReplica" => decode_kind!(DeploymentReplica, "deploymentReplica"),
        "SandboxClass" => decode_kind!(SandboxClass, "sandboxClass"),
        "SandboxPolicy" => decode_kind!(SandboxPolicy, "sandboxPolicy"),
        "Sandbox" => decode_kind!(Sandbox, "sandbox"),
        "Raw" => decode_kind!(Raw, "raw"),
        _ => {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "unsupported resource kind",
            ))
        }
    };
    Ok(spec)
}

async fn create_namespace(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<CreateNamespaceBody>,
) -> Response {
    let request = match tonic_request(
        &headers,
        proto::CreateNamespaceRequest {
            name: if body.name.is_empty() {
                name
            } else {
                body.name
            },
            recursive: body.recursive,
            labels: body.labels,
        },
    ) {
        Ok(request) => request,
        Err(response) => return response,
    };
    match handler(gateway).handle_create_namespace(request).await {
        Ok(response) => Json(namespace_json(response.into_inner())).into_response(),
        Err(status) => status_response(status),
    }
}

async fn get_namespace(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    let request = match tonic_request(&headers, proto::GetNamespaceRequest { name }) {
        Ok(request) => request,
        Err(response) => return response,
    };
    match handler(gateway).handle_get_namespace(request).await {
        Ok(response) => Json(namespace_json(response.into_inner())).into_response(),
        Err(status) => status_response(status),
    }
}

async fn list_namespaces(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Query(query): Query<ListNamespacesQuery>,
) -> Response {
    let request = match tonic_request(
        &headers,
        proto::ListNamespacesRequest {
            parent: query.parent,
        },
    ) {
        Ok(request) => request,
        Err(response) => return response,
    };
    match handler(gateway).handle_list_namespaces(request).await {
        Ok(response) => Json(json!({
            "namespaces": response
                .into_inner()
                .namespaces
                .into_iter()
                .map(namespace_json)
                .collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(status) => status_response(status),
    }
}

async fn create_resource(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path(ns): Path<String>,
    Json(body): Json<CreateResourceBody>,
) -> Response {
    let request = match tonic_request(
        &headers,
        proto::CreateResourceRequest {
            ns: if body.ns.is_empty() { ns } else { body.ns },
            manifest: Some(match resource_manifest_from_json(body.manifest) {
                Ok(manifest) => manifest,
                Err(response) => return response,
            }),
        },
    ) {
        Ok(request) => request,
        Err(response) => return response,
    };
    match handler(gateway).handle_create_resource(request).await {
        Ok(response) => Json(json!({ "resource": response.into_inner().resource })).into_response(),
        Err(status) => status_response(status),
    }
}

async fn get_resource(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path((ns, kind, name)): Path<(String, String, String)>,
) -> Response {
    get_resource_response(gateway, headers, ns, kind, name, "resource").await
}

async fn list_resources(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path(ns): Path<String>,
    Query(query): Query<ListResourcesQuery>,
) -> Response {
    let request = match tonic_request(
        &headers,
        proto::ListResourcesRequest {
            ns,
            kind: query.kind,
        },
    ) {
        Ok(request) => request,
        Err(response) => return response,
    };
    match handler(gateway).handle_list_resources(request).await {
        Ok(response) => Json(json!({
            "resources": response
                .into_inner()
                .resources
                .into_iter()
                .map(resource_json)
                .collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(status) => status_response(status),
    }
}

async fn get_mcp_server(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    get_resource_response(
        gateway,
        headers,
        crate::control::ns::TALON_SYSTEM.to_string(),
        "McpServer".to_string(),
        name,
        "server",
    )
    .await
}

async fn get_agent(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path((ns, name)): Path<(String, String)>,
) -> Response {
    get_resource_response(gateway, headers, ns, "Agent".to_string(), name, "agent").await
}

async fn get_mcp_binding(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path((ns, name)): Path<(String, String)>,
) -> Response {
    get_resource_response(
        gateway,
        headers,
        ns,
        "McpServerBinding".to_string(),
        name,
        "binding",
    )
    .await
}

async fn get_knowledge(
    State(gateway): State<Arc<Gateway>>,
    headers: HeaderMap,
    Path((ns, name)): Path<(String, String)>,
) -> Response {
    get_resource_response(
        gateway,
        headers,
        ns,
        "Knowledge".to_string(),
        name,
        "knowledge",
    )
    .await
}

async fn get_resource_response(
    gateway: Arc<Gateway>,
    headers: HeaderMap,
    ns: String,
    kind: String,
    name: String,
    response_key: &'static str,
) -> Response {
    let request = match tonic_request(
        &headers,
        proto::GetResourceRequest {
            ns,
            kind: kind.clone(),
            name,
        },
    ) {
        Ok(request) => request,
        Err(response) => return response,
    };
    match handler(gateway).handle_get_resource(request).await {
        Ok(response) => {
            let Some(resource) = response.into_inner().resource else {
                return json_error(StatusCode::NOT_FOUND, "resource not found");
            };
            if response_key == "resource" {
                Json(json!({ "resource": resource_json(resource) })).into_response()
            } else {
                match typed_resource_json(resource, &kind) {
                    Ok(value) => Json(json!({ response_key: value })).into_response(),
                    Err(response) => response,
                }
            }
        }
        Err(status) => status_response(status),
    }
}
