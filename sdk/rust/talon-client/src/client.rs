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

use crate::v1::{
    auth_service_client::AuthServiceClient, channel_service_client::ChannelServiceClient,
    knowledge_service_client::KnowledgeServiceClient,
    namespace_service_client::NamespaceServiceClient, resource_service_client::ResourceServiceClient,
    session_service_client::SessionServiceClient, workflow_service_client::WorkflowServiceClient,
};

type BoxError = Box<dyn Error + Send + Sync + 'static>;
pub type NativeService =
    tonic::service::interceptor::InterceptedService<tonic::transport::Channel, AuthInterceptor>;
type GrpcWebBody = http_body_util::combinators::BoxBody<Bytes, reqwest_grpc_web::Error>;
pub type GrpcWebService = tonic_web::GrpcWebClientService<GrpcWebHttpService>;

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
pub struct TalonClientset<T> {
    pub namespaces: NamespaceServiceClient<T>,
    pub resources: ResourceServiceClient<T>,
    pub sessions: SessionServiceClient<T>,
    pub channels: ChannelServiceClient<T>,
    pub workflows: WorkflowServiceClient<T>,
    pub knowledge: KnowledgeServiceClient<T>,
    pub auth: AuthServiceClient<T>,
}

pub type NativeTalonClient = TalonClientset<NativeService>;
pub type GrpcWebTalonClient = TalonClientset<GrpcWebService>;

#[derive(Debug)]
pub enum TalonClient {
    Native(NativeTalonClient),
    GrpcWeb(GrpcWebTalonClient),
}

impl NativeTalonClient {
    pub async fn connect(endpoint: impl Into<String>) -> Result<Self, BoxError> {
        Self::connect_with_options(GatewayClientOptions::new(endpoint)).await
    }

    pub async fn connect_with_options(options: GatewayClientOptions) -> Result<Self, BoxError> {
        connect_native(options).await
    }
}

impl GrpcWebTalonClient {
    pub fn connect_grpc_web(endpoint: impl Into<String>) -> Result<Self, BoxError> {
        let mut options = GatewayClientOptions::new(endpoint);
        options.transport = GatewayTransport::GrpcWeb;
        Self::connect_grpc_web_with_options(options)
    }

    pub fn connect_grpc_web_with_options(options: GatewayClientOptions) -> Result<Self, BoxError> {
        connect_grpc_web(options)
    }
}

macro_rules! delegate_dynamic_rpc {
    ($name:ident, $field:ident, $method:ident, $request:ty, $response:ty) => {
        pub async fn $name(
            &mut self,
            request: $request,
        ) -> Result<tonic::Response<$response>, tonic::Status> {
            match self {
                TalonClient::Native(client) => client.$field.$method(request).await,
                TalonClient::GrpcWeb(client) => client.$field.$method(request).await,
            }
        }
    };
}

impl TalonClient {
    pub async fn connect_with_options(options: GatewayClientOptions) -> Result<Self, BoxError> {
        match options.transport {
            GatewayTransport::Grpc => Ok(Self::Native(connect_native(options).await?)),
            GatewayTransport::GrpcWeb => Ok(Self::GrpcWeb(connect_grpc_web(options)?)),
        }
    }

    delegate_dynamic_rpc!(
        create_namespace,
        namespaces,
        create,
        crate::v1::CreateNamespaceRequest,
        crate::v1::NamespaceResponse
    );
    delegate_dynamic_rpc!(
        create_resource,
        resources,
        create,
        crate::v1::CreateResourceRequest,
        crate::v1::ResourceResponse
    );
    delegate_dynamic_rpc!(
        get_resource,
        resources,
        get,
        crate::v1::GetResourceRequest,
        crate::v1::ResourceResponse
    );
    delegate_dynamic_rpc!(
        list_resources,
        resources,
        list,
        crate::v1::ListResourcesRequest,
        crate::v1::ListResourcesResponse
    );
    delegate_dynamic_rpc!(
        delete_resource,
        resources,
        delete,
        crate::v1::DeleteResourceRequest,
        crate::v1::DeleteResourceResponse
    );
    delegate_dynamic_rpc!(
        list_namespaces,
        namespaces,
        list,
        crate::v1::ListNamespacesRequest,
        crate::v1::ListNamespacesResponse
    );
    delegate_dynamic_rpc!(
        create_session,
        sessions,
        create,
        crate::v1::CreateSessionRequest,
        crate::v1::SessionResponse
    );
    delegate_dynamic_rpc!(
        send_message,
        sessions,
        send_message,
        crate::v1::SendMessageRequest,
        crate::v1::SendMessageResponse
    );
    delegate_dynamic_rpc!(
        get_session,
        sessions,
        get,
        crate::v1::GetSessionRequest,
        crate::v1::SessionResponse
    );
    delegate_dynamic_rpc!(
        list_sessions,
        sessions,
        list,
        crate::v1::ListSessionsRequest,
        crate::v1::ListSessionsResponse
    );
    delegate_dynamic_rpc!(
        list_session_messages,
        sessions,
        list_messages,
        crate::v1::ListSessionMessagesRequest,
        crate::v1::ListSessionMessagesResponse
    );
    delegate_dynamic_rpc!(
        answer_session_permission,
        sessions,
        answer_permission,
        crate::v1::AnswerSessionPermissionRequest,
        crate::v1::AnswerSessionPermissionResponse
    );
    delegate_dynamic_rpc!(
        stop_session_generation,
        sessions,
        stop_generation,
        crate::v1::StopSessionGenerationRequest,
        crate::v1::StopSessionGenerationResponse
    );
    delegate_dynamic_rpc!(
        clear_session,
        sessions,
        clear,
        crate::v1::ClearSessionRequest,
        crate::v1::ClearSessionResponse
    );
    delegate_dynamic_rpc!(
        delete_session,
        sessions,
        delete,
        crate::v1::DeleteSessionRequest,
        crate::v1::DeleteSessionResponse
    );
    delegate_dynamic_rpc!(
        create_workflow_run,
        workflows,
        create_run,
        crate::v1::CreateWorkflowRunRequest,
        crate::v1::WorkflowRunResponse
    );
    delegate_dynamic_rpc!(
        get_workflow_run,
        workflows,
        get_run,
        crate::v1::GetWorkflowRunRequest,
        crate::v1::WorkflowRunResponse
    );
    delegate_dynamic_rpc!(
        list_workflow_runs,
        workflows,
        list_runs,
        crate::v1::ListWorkflowRunsRequest,
        crate::v1::ListWorkflowRunsResponse
    );
    delegate_dynamic_rpc!(
        resume_workflow_run,
        workflows,
        resume_run,
        crate::v1::ResumeWorkflowRunRequest,
        crate::v1::WorkflowRunResponse
    );
    delegate_dynamic_rpc!(
        cancel_workflow_run,
        workflows,
        cancel_run,
        crate::v1::CancelWorkflowRunRequest,
        crate::v1::WorkflowRunResponse
    );
    delegate_dynamic_rpc!(
        get_knowledge,
        knowledge,
        get,
        crate::v1::GetKnowledgeRequest,
        crate::v1::KnowledgeResponse
    );
    delegate_dynamic_rpc!(
        search_knowledge,
        knowledge,
        search,
        crate::v1::SearchKnowledgeRequest,
        crate::v1::SearchKnowledgeResponse
    );
    delegate_dynamic_rpc!(
        get_sso_config,
        auth,
        get_sso_config,
        crate::v1::GetSsoConfigRequest,
        crate::v1::GetSsoConfigResponse
    );
    delegate_dynamic_rpc!(
        exchange_oidc_token,
        auth,
        exchange_oidc_token,
        crate::v1::ExchangeOidcTokenRequest,
        crate::v1::ExchangeOidcTokenResponse
    );

    pub async fn stream_session_parts(
        &mut self,
        request: crate::v1::StreamSessionPartsRequest,
    ) -> Result<tonic::Response<tonic::codec::Streaming<crate::events::SessionMessagePartEvent>>, tonic::Status>
    {
        match self {
            TalonClient::Native(client) => client.sessions.stream_parts(request).await,
            TalonClient::GrpcWeb(client) => client.sessions.stream_parts(request).await,
        }
    }

    pub async fn stream_workflow_events(
        &mut self,
        request: crate::v1::StreamWorkflowEventsRequest,
    ) -> Result<tonic::Response<tonic::codec::Streaming<crate::data::WorkflowRunEvent>>, tonic::Status>
    {
        match self {
            TalonClient::Native(client) => client.workflows.stream_events(request).await,
            TalonClient::GrpcWeb(client) => client.workflows.stream_events(request).await,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AuthInterceptor {
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

async fn connect_native(options: GatewayClientOptions) -> Result<NativeTalonClient, BoxError> {
    let mut endpoint = tonic::transport::Endpoint::from_shared(options.endpoint)?;
    if let Some(timeout) = options.connect_timeout {
        endpoint = endpoint.connect_timeout(timeout);
    }
    if let Some(timeout) = options.request_timeout {
        endpoint = endpoint.timeout(timeout);
    }
    let channel = endpoint.connect().await?;
    let service =
        tonic::service::interceptor::InterceptedService::new(channel, AuthInterceptor::new(options.authorization)?);
    Ok(TalonClientset {
        namespaces: NamespaceServiceClient::new(service.clone()),
        resources: ResourceServiceClient::new(service.clone()),
        sessions: SessionServiceClient::new(service.clone()),
        channels: ChannelServiceClient::new(service.clone()),
        workflows: WorkflowServiceClient::new(service.clone()),
        knowledge: KnowledgeServiceClient::new(service.clone()),
        auth: AuthServiceClient::new(service),
    })
}

fn connect_grpc_web(options: GatewayClientOptions) -> Result<GrpcWebTalonClient, BoxError> {
    let service = tonic_web::GrpcWebClientService::new(GrpcWebHttpService::new(options)?);
    Ok(TalonClientset {
        namespaces: NamespaceServiceClient::new(service.clone()),
        resources: ResourceServiceClient::new(service.clone()),
        sessions: SessionServiceClient::new(service.clone()),
        channels: ChannelServiceClient::new(service.clone()),
        workflows: WorkflowServiceClient::new(service.clone()),
        knowledge: KnowledgeServiceClient::new(service.clone()),
        auth: AuthServiceClient::new(service),
    })
}

#[derive(Clone, Debug)]
pub struct GrpcWebHttpService {
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
