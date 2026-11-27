// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[allow(unused_imports)]
use crate::talon;

use talon::control::keys;

use talon::control::ns;

use hyper_util::rt::TokioIo;
use prost::Message;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use talon::control::KeyValueStore;
use talon::protocol::resources_proto;
use talon::protocol::worker_proto;
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use url::Url;

#[derive(Clone, Debug)]
pub struct WorkerGrpcAuthInterceptor {
    require_platform_jwt: bool,
}

impl tonic::service::Interceptor for WorkerGrpcAuthInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> std::result::Result<tonic::Request<()>, tonic::Status> {
        let token = match (
            talon::security::platform_jwt::load_key(),
            talon::security::platform_jwt::issuer(),
        ) {
            (Ok(key), Ok(issuer)) => talon::security::platform_jwt::mint_worker_grpc_token(
                &key,
                &issuer,
                "talon-gateway",
                talon::security::platform_jwt::DEFAULT_WORKER_GRPC_TOKEN_TTL_SECONDS,
            )
            .map_err(|err| {
                tonic::Status::internal(format!("failed to mint worker gRPC auth token: {err}"))
            })?,
            (Err(err), _) | (_, Err(err)) => {
                if self.require_platform_jwt {
                    return Err(tonic::Status::internal(format!(
                        "worker gRPC auth is not configured: {err}"
                    )));
                }
                return Ok(request);
            }
        };
        let header_value = tonic::metadata::MetadataValue::try_from(format!("Bearer {token}"))
            .map_err(|err| {
                tonic::Status::internal(format!("failed to encode worker gRPC auth header: {err}"))
            })?;
        request.metadata_mut().insert("authorization", header_value);
        Ok(request)
    }
}

pub struct WorkerConnectionPool {
    channels: Mutex<HashMap<String, Channel>>,
    require_platform_jwt: bool,
}

impl Default for WorkerConnectionPool {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerConnectionPool {
    pub fn new() -> Self {
        Self {
            channels: Mutex::new(HashMap::new()),
            require_platform_jwt: true,
        }
    }

    pub(crate) fn new_unauthenticated() -> Self {
        Self {
            channels: Mutex::new(HashMap::new()),
            require_platform_jwt: false,
        }
    }

    fn auth_interceptor(&self) -> WorkerGrpcAuthInterceptor {
        WorkerGrpcAuthInterceptor {
            require_platform_jwt: self.require_platform_jwt,
        }
    }

    pub async fn worker_endpoints(
        kv: &dyn KeyValueStore,
        worker_id: &str,
    ) -> std::result::Result<Vec<resources_proto::WorkerEndpoint>, tonic::Status> {
        let key = keys::ResourceKey::new(ns::TALON_SYSTEM, &[], "Worker", worker_id);
        let Some(bytes) = kv
            .get(&key)
            .await
            .map_err(|err| tonic::Status::internal(format!("Failed to fetch worker: {err}")))?
        else {
            return Err(tonic::Status::not_found("claim worker not found"));
        };
        let worker = resources_proto::Worker::decode(bytes.as_slice())
            .map_err(|err| tonic::Status::internal(format!("Failed to decode worker: {err}")))?;
        let Some(status) = worker.status else {
            return Err(tonic::Status::unavailable("worker status is missing"));
        };
        if status.phase != "ready" {
            return Err(tonic::Status::unavailable("worker is not ready"));
        }
        let endpoints: Vec<_> = status
            .endpoints
            .into_iter()
            .filter(|endpoint| !endpoint.url.trim().is_empty())
            .collect();
        if endpoints.is_empty() {
            return Err(tonic::Status::unavailable("worker has no endpoints"));
        }
        Ok(endpoints)
    }

    pub async fn fanout_client(
        &self,
        endpoint: &resources_proto::WorkerEndpoint,
    ) -> std::result::Result<
        worker_proto::fanout_service_client::FanoutServiceClient<
            InterceptedService<Channel, WorkerGrpcAuthInterceptor>,
        >,
        tonic::Status,
    > {
        Ok(
            worker_proto::fanout_service_client::FanoutServiceClient::with_interceptor(
                self.channel(endpoint).await?,
                self.auth_interceptor(),
            ),
        )
    }

    pub async fn session_control_client(
        &self,
        endpoint: &resources_proto::WorkerEndpoint,
    ) -> std::result::Result<
        worker_proto::session_control_service_client::SessionControlServiceClient<
            InterceptedService<Channel, WorkerGrpcAuthInterceptor>,
        >,
        tonic::Status,
    > {
        Ok(
            worker_proto::session_control_service_client::SessionControlServiceClient::with_interceptor(
                self.channel(endpoint).await?,
                self.auth_interceptor(),
            ),
        )
    }

    async fn channel(
        &self,
        endpoint: &resources_proto::WorkerEndpoint,
    ) -> std::result::Result<Channel, tonic::Status> {
        let url = endpoint.url.clone();
        if let Some(channel) = self.channels.lock().await.get(&url).cloned() {
            return Ok(channel);
        }

        let parsed = Url::parse(&url)
            .map_err(|err| tonic::Status::unavailable(format!("invalid worker endpoint: {err}")))?;
        let channel = if parsed.scheme() == "unix" {
            if parsed.path().is_empty() {
                return Err(tonic::Status::unavailable(
                    "unix worker endpoint is missing a socket path",
                ));
            }
            let path = urlencoding::decode(parsed.path()).map_err(|err| {
                tonic::Status::unavailable(format!("invalid unix worker endpoint path: {err}"))
            })?;
            let path = Arc::new(PathBuf::from(path.into_owned()));
            Endpoint::try_from("http://[::]:50051")
                .map_err(|err| {
                    tonic::Status::unavailable(format!("invalid worker endpoint: {err}"))
                })?
                .connect_with_connector(service_fn(move |_uri: Uri| {
                    let path = path.clone();
                    async move { UnixStream::connect(path.as_ref()).await.map(TokioIo::new) }
                }))
                .await
                .map_err(|err| {
                    tonic::Status::unavailable(format!("failed to connect to worker: {err}"))
                })?
        } else {
            Channel::from_shared(url.clone())
                .map_err(|err| {
                    tonic::Status::unavailable(format!("invalid worker endpoint: {err}"))
                })?
                .connect()
                .await
                .map_err(|err| {
                    tonic::Status::unavailable(format!("failed to connect to worker: {err}"))
                })?
        };
        let channel = self
            .channels
            .lock()
            .await
            .entry(url)
            .or_insert_with(|| channel.clone())
            .clone();
        Ok(channel)
    }

    #[cfg(test)]
    pub async fn cached_channel_count(&self) -> usize {
        self.channels.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talon::security::platform_jwt;
    use talon::session_control::{SessionCancellationRegistry, SessionControlServiceImpl};
    use tempfile::tempdir;
    use tokio::net::UnixListener;
    use tokio_stream::wrappers::UnixListenerStream;
    use tokio_util::sync::CancellationToken;
    use tonic::transport::Server;

    const TEST_RSA_PRIVATE_KEY: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../src/control/security/test_rsa_private_key.pem"
    ));

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(previous) = &self.previous {
                    std::env::set_var(self.key, previous);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    fn require_worker_grpc_auth(
        request: tonic::Request<()>,
    ) -> std::result::Result<tonic::Request<()>, tonic::Status> {
        let auth_header = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok());
        platform_jwt::authorize_worker_grpc_authorization_header(auth_header)
            .map_err(|_| tonic::Status::unauthenticated("unauthorized worker gRPC request"))?;
        Ok(request)
    }

    #[tokio::test]
    async fn authenticated_pool_attaches_valid_worker_grpc_jwt() {
        let _env_lock = crate::test_support::async_env_mutex().lock().await;
        let _key = EnvGuard::set(
            platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
            TEST_RSA_PRIVATE_KEY,
        );
        let _issuer = EnvGuard::set(
            platform_jwt::TALON_JWT_ISSUER_ENV,
            "https://talon.localhost",
        );

        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("worker-auth.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let shutdown = CancellationToken::new();
        let registry = Arc::new(SessionCancellationRegistry::default());
        let service =
            worker_proto::session_control_service_server::SessionControlServiceServer::with_interceptor(
                SessionControlServiceImpl::new(registry),
                require_worker_grpc_auth,
            );
        let server = tokio::spawn(
            Server::builder()
                .add_service(service)
                .serve_with_incoming_shutdown(
                    UnixListenerStream::new(listener),
                    shutdown.clone().cancelled_owned(),
                ),
        );

        let endpoint = resources_proto::WorkerEndpoint {
            url: format!("unix://{}", socket_path.display()),
            protocol: "grpc".to_string(),
            audience: String::new(),
        };

        let pool = WorkerConnectionPool::new();
        let mut client = pool.session_control_client(&endpoint).await.unwrap();
        let response = client
            .cancel_session_generation(worker_proto::CancelSessionGenerationRequest {
                ns: "default".into(),
                agent: "assistant".into(),
                session_id: "s1".into(),
                submission_id: "sub1".into(),
                attempt_id: "att1".into(),
            })
            .await
            .expect("authenticated worker gRPC call should succeed");
        assert!(!response.into_inner().cancelled);

        // Raw unauthenticated client must be rejected by the interceptor.
        let raw_channel = pool.channel(&endpoint).await.unwrap();
        let mut raw_client =
            worker_proto::session_control_service_client::SessionControlServiceClient::new(
                raw_channel,
            );
        let err = raw_client
            .cancel_session_generation(worker_proto::CancelSessionGenerationRequest::default())
            .await
            .expect_err("unauthenticated worker gRPC call must fail");
        assert_eq!(err.code(), tonic::Code::Unauthenticated);

        shutdown.cancel();
        let _ = server.await;
    }
}
