// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use futures::lock::Mutex;
use futures::TryStreamExt;
use http_body::Frame;
use http_body_util::{BodyExt, StreamBody};
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tonic::codegen::{http, Body, Bytes, Service, StdError};
use tonic::transport::Channel;

use crate::TalonClientset;

type BoxError = Box<dyn Error + Send + Sync + 'static>;
pub type NativeService = AuthenticatedNativeService;
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
    pub api_key: Option<String>,
    pub connect_timeout: Option<Duration>,
    pub request_timeout: Option<Duration>,
}

impl GatewayClientOptions {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            transport: GatewayTransport::Grpc,
            authorization: None,
            api_key: None,
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
pub struct AuthenticatedNativeService {
    inner: Channel,
    authorization: Option<http::HeaderValue>,
    token_source: Option<Arc<ApiKeyTokenSource>>,
}

impl AuthenticatedNativeService {
    fn new(
        inner: Channel,
        authorization: Option<String>,
        token_source: Option<Arc<ApiKeyTokenSource>>,
    ) -> Result<Self, BoxError> {
        Ok(Self {
            inner,
            authorization: authorization
                .map(http::HeaderValue::try_from)
                .transpose()
                .map_err(|err| -> BoxError { Box::new(err) })?,
            token_source,
        })
    }
}

impl Service<http::Request<tonic::body::BoxBody>> for AuthenticatedNativeService {
    type Response = <Channel as Service<http::Request<tonic::body::BoxBody>>>::Response;
    type Error = StdError;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, mut req: http::Request<tonic::body::BoxBody>) -> Self::Future {
        let authorization = self.authorization.clone();
        let token_source = self.token_source.clone();
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        Box::pin(async move {
            if let Some(auth) = authorization {
                req.headers_mut().insert("authorization", auth);
            } else if let Some(token_source) = token_source {
                let auth = token_source.authorization().await?;
                req.headers_mut().insert("authorization", auth);
            }
            inner.call(req).await.map_err(Into::into)
        })
    }
}

#[derive(Clone, Debug)]
struct ApiKeyTokenSource {
    channel: Channel,
    api_key: String,
    cached: Arc<Mutex<CachedApiKeyToken>>,
    refresh_lock: Arc<Mutex<()>>,
    refresh_skew: u64,
}

#[derive(Debug, Default)]
struct CachedApiKeyToken {
    token: Option<http::HeaderValue>,
    expires_at: u64,
}

impl ApiKeyTokenSource {
    fn new(channel: Channel, api_key: String) -> Self {
        Self {
            channel,
            api_key,
            cached: Arc::new(Mutex::new(CachedApiKeyToken::default())),
            refresh_lock: Arc::new(Mutex::new(())),
            refresh_skew: 60,
        }
    }

    async fn authorization(&self) -> Result<http::HeaderValue, StdError> {
        let now = unix_timestamp();
        {
            let cached = self.cached.lock().await;
            if let Some(token) = &cached.token {
                if cached.expires_at > now + self.refresh_skew {
                    return Ok(token.clone());
                }
            }
        }

        let _refresh_guard = self.refresh_lock.lock().await;
        let now = unix_timestamp();
        {
            let cached = self.cached.lock().await;
            if let Some(token) = &cached.token {
                if cached.expires_at > now + self.refresh_skew {
                    return Ok(token.clone());
                }
            }
        }

        let mut client =
            crate::v1::auth_service_client::AuthServiceClient::new(self.channel.clone());
        let exchanged = client
            .exchange_api_key(crate::v1::ExchangeApiKeyRequest {
                api_key: self.api_key.clone(),
                grant: None,
                expires_in: 0,
            })
            .await?
            .into_inner();
        let token = http::HeaderValue::try_from(format!("Bearer {}", exchanged.access_token))?;
        let mut cached = self.cached.lock().await;
        cached.expires_at = exchanged.expires_at.try_into().unwrap_or_default();
        cached.token = Some(token.clone());
        Ok(token)
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn connect_native(options: GatewayClientOptions) -> Result<NativeTalonClient, BoxError> {
    let endpoint_url = endpoint_url(&options.endpoint);
    if options
        .api_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        if endpoint_url.starts_with("http://") {
            return Err("api_key auth requires an HTTPS native gRPC endpoint".into());
        }
    }
    let mut endpoint = tonic::transport::Endpoint::new(endpoint_url)?;
    if let Some(timeout) = options.connect_timeout {
        endpoint = endpoint.connect_timeout(timeout);
    }
    if let Some(timeout) = options.request_timeout {
        endpoint = endpoint.timeout(timeout);
    }
    let channel = endpoint.connect().await?;
    let token_source = options
        .api_key
        .as_deref()
        .filter(|_| options.authorization.is_none())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|api_key| Arc::new(ApiKeyTokenSource::new(channel.clone(), api_key.to_string())));
    let service = AuthenticatedNativeService::new(channel, options.authorization, token_source)?;
    Ok(TalonClientset::from_service(service))
}

fn endpoint_url(endpoint: &str) -> String {
    let endpoint = endpoint.trim();
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        return endpoint.to_string();
    }
    if endpoint.starts_with("localhost:")
        || endpoint.starts_with("127.")
        || endpoint.starts_with("0.0.0.0:")
        || endpoint.starts_with("[::1]:")
    {
        return format!("http://{endpoint}");
    }
    format!("https://{endpoint}")
}

#[cfg(test)]
mod tests {
    use super::{endpoint_url, GatewayClientOptions, NativeTalonClient};

    #[test]
    fn endpoint_url_defaults_hosted_gateways_to_https() {
        assert_eq!(
            endpoint_url("talon.impala.systems"),
            "https://talon.impala.systems"
        );
        assert_eq!(
            endpoint_url(" https://talon.impala.systems:443 "),
            "https://talon.impala.systems:443"
        );
    }

    #[test]
    fn endpoint_url_keeps_local_gateways_on_http() {
        assert_eq!(
            endpoint_url("localhost:50051"),
            "http://localhost:50051"
        );
        assert_eq!(
            endpoint_url("127.0.0.1:50051"),
            "http://127.0.0.1:50051"
        );
        assert_eq!(endpoint_url("[::1]:50051"), "http://[::1]:50051");
    }

    #[tokio::test]
    async fn native_api_key_auth_rejects_plaintext_endpoints() {
        let mut options = GatewayClientOptions::new("http://127.0.0.1:50051");
        options.api_key = Some("talon_sk_v1_id_secret".to_string());
        let err = NativeTalonClient::connect_with_options(options)
            .await
            .expect_err("plaintext api_key auth should be rejected");
        assert!(err.to_string().contains("requires an HTTPS"));
    }
}

fn connect_grpc_web(options: GatewayClientOptions) -> Result<GrpcWebTalonClient, BoxError> {
    if options
        .api_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Err("api_key auth is only supported by the native async Rust client".into());
    }
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
            endpoint: endpoint_url(&options.endpoint)
                .trim_end_matches('/')
                .to_string(),
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
