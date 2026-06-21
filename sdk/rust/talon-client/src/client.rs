// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use futures::TryStreamExt;
use http_body::Frame;
use http_body_util::{BodyExt, StreamBody};
use std::error::Error;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tonic::codegen::{http, Body, Bytes, Service, StdError};
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::{Request, Status};

use crate::gateway::gateway_service_client::GatewayServiceClient;

type BoxError = Box<dyn Error + Send + Sync + 'static>;
type NativeGatewayClient = GatewayServiceClient<
    tonic::service::interceptor::InterceptedService<tonic::transport::Channel, AuthInterceptor>,
>;
type GrpcWebBody = http_body_util::combinators::BoxBody<Bytes, reqwest_grpc_web::Error>;
type GrpcWebGatewayClient =
    GatewayServiceClient<tonic_web::GrpcWebClientService<GrpcWebHttpService>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayTransport {
    Grpc,
    GrpcWeb,
}

#[derive(Clone, Debug)]
pub struct GatewayClientOptions {
    pub endpoint: String,
    pub transport: GatewayTransport,
    pub authorization: Option<String>,
    pub connect_timeout: Option<Duration>,
    pub request_timeout: Option<Duration>,
}

impl GatewayClientOptions {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            transport: GatewayTransport::Grpc,
            authorization: None,
            connect_timeout: Some(Duration::from_secs(5)),
            request_timeout: Some(Duration::from_secs(30)),
        }
    }
}

#[derive(Debug)]
pub struct TalonGatewayClient {
    inner: TalonGatewayClientInner,
}

#[derive(Debug)]
enum TalonGatewayClientInner {
    Native(NativeGatewayClient),
    GrpcWeb(GrpcWebGatewayClient),
}

macro_rules! delegate_gateway_rpc {
    ($name:ident, $request:ty, $response:ty) => {
        pub async fn $name(
            &mut self,
            request: $request,
        ) -> Result<tonic::Response<$response>, tonic::Status> {
            match &mut self.inner {
                TalonGatewayClientInner::Native(client) => client.$name(request).await,
                TalonGatewayClientInner::GrpcWeb(client) => client.$name(request).await,
            }
        }
    };
}

impl TalonGatewayClient {
    pub async fn connect(endpoint: impl Into<String>) -> Result<Self, BoxError> {
        Self::connect_with_options(GatewayClientOptions::new(endpoint)).await
    }

    pub async fn connect_grpc_web(endpoint: impl Into<String>) -> Result<Self, BoxError> {
        let mut options = GatewayClientOptions::new(endpoint);
        options.transport = GatewayTransport::GrpcWeb;
        Self::connect_with_options(options).await
    }

    pub async fn connect_with_options(options: GatewayClientOptions) -> Result<Self, BoxError> {
        match options.transport {
            GatewayTransport::Grpc => connect_native(options).await,
            GatewayTransport::GrpcWeb => connect_grpc_web(options),
        }
    }

    delegate_gateway_rpc!(
        create_namespace,
        crate::gateway::CreateNamespaceRequest,
        crate::gateway::NamespaceResponse
    );
    delegate_gateway_rpc!(
        create_resource,
        crate::gateway::CreateResourceRequest,
        crate::gateway::ResourceResponse
    );
    delegate_gateway_rpc!(
        get_resource,
        crate::gateway::GetResourceRequest,
        crate::gateway::ResourceResponse
    );
    delegate_gateway_rpc!(
        list_resources,
        crate::gateway::ListResourcesRequest,
        crate::gateway::ListResourcesResponse
    );
    delegate_gateway_rpc!(
        delete_resource,
        crate::gateway::DeleteResourceRequest,
        crate::gateway::DeleteResourceResponse
    );
    delegate_gateway_rpc!(
        list_namespaces,
        crate::gateway::ListNamespacesRequest,
        crate::gateway::ListNamespacesResponse
    );
    delegate_gateway_rpc!(
        create_session,
        crate::gateway::CreateSessionRequest,
        crate::gateway::SessionResponse
    );
    delegate_gateway_rpc!(
        send_message,
        crate::gateway::SendMessageRequest,
        crate::gateway::SendMessageResponse
    );
    delegate_gateway_rpc!(
        stream_session_parts,
        crate::gateway::StreamSessionPartsRequest,
        tonic::codec::Streaming<crate::events::SessionMessagePartEvent>
    );
    delegate_gateway_rpc!(
        get_session,
        crate::gateway::GetSessionRequest,
        crate::gateway::SessionResponse
    );
    delegate_gateway_rpc!(
        list_sessions,
        crate::gateway::ListSessionsRequest,
        crate::gateway::ListSessionsResponse
    );
    delegate_gateway_rpc!(
        list_session_messages,
        crate::gateway::ListSessionMessagesRequest,
        crate::gateway::ListSessionMessagesResponse
    );
    delegate_gateway_rpc!(
        answer_session_permission,
        crate::gateway::AnswerSessionPermissionRequest,
        crate::gateway::AnswerSessionPermissionResponse
    );
    delegate_gateway_rpc!(
        stop_session_generation,
        crate::gateway::StopSessionGenerationRequest,
        crate::gateway::StopSessionGenerationResponse
    );
    delegate_gateway_rpc!(
        clear_session,
        crate::gateway::ClearSessionRequest,
        crate::gateway::ClearSessionResponse
    );
    delegate_gateway_rpc!(
        delete_session,
        crate::gateway::DeleteSessionRequest,
        crate::gateway::DeleteSessionResponse
    );
    delegate_gateway_rpc!(
        create_workflow_run,
        crate::gateway::CreateWorkflowRunRequest,
        crate::gateway::WorkflowRunResponse
    );
    delegate_gateway_rpc!(
        get_workflow_run,
        crate::gateway::GetWorkflowRunRequest,
        crate::gateway::WorkflowRunResponse
    );
    delegate_gateway_rpc!(
        list_workflow_runs,
        crate::gateway::ListWorkflowRunsRequest,
        crate::gateway::ListWorkflowRunsResponse
    );
    delegate_gateway_rpc!(
        resume_workflow_run,
        crate::gateway::ResumeWorkflowRunRequest,
        crate::gateway::WorkflowRunResponse
    );
    delegate_gateway_rpc!(
        cancel_workflow_run,
        crate::gateway::CancelWorkflowRunRequest,
        crate::gateway::WorkflowRunResponse
    );
    delegate_gateway_rpc!(
        stream_workflow_events,
        crate::gateway::StreamWorkflowEventsRequest,
        tonic::codec::Streaming<crate::data::WorkflowRunEvent>
    );
}

#[derive(Clone, Debug)]
struct AuthInterceptor {
    authorization: Option<MetadataValue<tonic::metadata::Ascii>>,
}

impl AuthInterceptor {
    fn new(authorization: Option<String>) -> Result<Self, BoxError> {
        Ok(Self {
            authorization: authorization
                .map(MetadataValue::try_from)
                .transpose()
                .map_err(|err| -> BoxError { Box::new(err) })?,
        })
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        if let Some(auth) = &self.authorization {
            req.metadata_mut().insert("authorization", auth.clone());
        }
        Ok(req)
    }
}

async fn connect_native(options: GatewayClientOptions) -> Result<TalonGatewayClient, BoxError> {
    let mut endpoint = tonic::transport::Endpoint::from_shared(options.endpoint)?;
    if let Some(timeout) = options.connect_timeout {
        endpoint = endpoint.connect_timeout(timeout);
    }
    if let Some(timeout) = options.request_timeout {
        endpoint = endpoint.timeout(timeout);
    }
    let channel = endpoint.connect().await?;
    Ok(TalonGatewayClient {
        inner: TalonGatewayClientInner::Native(GatewayServiceClient::with_interceptor(
            channel,
            AuthInterceptor::new(options.authorization)?,
        )),
    })
}

fn connect_grpc_web(options: GatewayClientOptions) -> Result<TalonGatewayClient, BoxError> {
    let service = tonic_web::GrpcWebClientService::new(GrpcWebHttpService::new(options)?);
    Ok(TalonGatewayClient {
        inner: TalonGatewayClientInner::GrpcWeb(GatewayServiceClient::new(service)),
    })
}

#[derive(Clone, Debug)]
struct GrpcWebHttpService {
    client: reqwest_grpc_web::Client,
    endpoint: String,
    authorization: Option<String>,
}

impl GrpcWebHttpService {
    fn new(options: GatewayClientOptions) -> Result<Self, BoxError> {
        let mut builder = reqwest_grpc_web::Client::builder();
        if let Some(timeout) = options.connect_timeout {
            builder = builder.connect_timeout(timeout);
        }
        if let Some(timeout) = options.request_timeout {
            builder = builder.timeout(timeout);
        }
        Ok(Self {
            client: builder.build()?,
            endpoint: options.endpoint.trim_end_matches('/').to_string(),
            authorization: options.authorization,
        })
    }
}

impl<B> Service<http::Request<B>> for GrpcWebHttpService
where
    B: Body<Data = Bytes> + Send + 'static,
    B::Error: Into<StdError> + Send + Sync + 'static,
{
    type Response = http::Response<GrpcWebBody>;
    type Error = StdError;
    type Future = Pin<
        Box<
            dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send + 'static,
        >,
    >;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let client = self.client.clone();
        let endpoint = self.endpoint.clone();
        let authorization = self.authorization.clone();
        Box::pin(async move {
            let (parts, body) = req.into_parts();
            let path = parts
                .uri
                .path_and_query()
                .map(|value| value.as_str())
                .unwrap_or("/");
            let url = format!("{endpoint}{path}");
            let body = body.collect().await.map_err(Into::into)?.to_bytes();
            let method = reqwest_grpc_web::Method::from_bytes(parts.method.as_str().as_bytes())
                .map_err(|err| -> StdError { Box::new(err) })?;
            let mut request = client.request(method, url).body(body);
            for (name, value) in parts.headers.iter() {
                if name != http::header::HOST {
                    request = request.header(name.as_str(), value.as_bytes());
                }
            }
            request = request
                .header("x-grpc-web", "1")
                .header("x-user-agent", "talon-client-rust");
            if let Some(authorization) = authorization {
                request = request.header("authorization", authorization);
            }

            let response = request
                .send()
                .await
                .map_err(|err| -> StdError { Box::new(err) })?;
            let mut builder = http::Response::builder().status(response.status().as_u16());
            for (name, value) in response.headers() {
                builder = builder.header(name.as_str(), value.as_bytes());
            }
            let body = StreamBody::new(response.bytes_stream().map_ok(Frame::data)).boxed();
            builder
                .body(body)
                .map_err(|err| -> StdError { Box::new(err) })
        })
    }
}
