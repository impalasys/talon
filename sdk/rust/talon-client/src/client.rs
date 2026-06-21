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

use crate::TalonClientset;

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

impl TalonClient {
    pub async fn connect_with_options(options: GatewayClientOptions) -> Result<Self, BoxError> {
        match options.transport {
            GatewayTransport::Grpc => Ok(Self::Native(connect_native(options).await?)),
            GatewayTransport::GrpcWeb => Ok(Self::GrpcWeb(connect_grpc_web(options)?)),
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
    let service = tonic::service::interceptor::InterceptedService::new(
        channel,
        AuthInterceptor::new(options.authorization)?,
    );
    Ok(TalonClientset::from_service(service))
}

fn connect_grpc_web(options: GatewayClientOptions) -> Result<GrpcWebTalonClient, BoxError> {
    let service = tonic_web::GrpcWebClientService::new(GrpcWebHttpService::new(options)?);
    Ok(TalonClientset::from_service(service))
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
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>,
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
