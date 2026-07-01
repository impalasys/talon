// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use futures::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use talon::{
    control::{
        build_control_plane,
        config::{Config, ConfigExt},
        topics, ControlPlane, MessagePublisher,
    },
    gateway::rpc::resources_proto,
    gateway::{auth::AuthConfig, server::Gateway},
    worker::{
        fanout::FanoutHub, mcp_registry::McpRegistry,
        scheduler_auth::SchedulerRequestAuthenticator, WorkerEventHandler,
    },
};
use tokio::{net::UnixListener, signal, task::JoinHandle};
use tokio_stream::wrappers::UnixListenerStream;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;
use tracing::Instrument;
use url::Url;

#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

#[cfg(feature = "heap-profile")]
#[global_allocator]
static GLOBAL_ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn gateway_addr() -> String {
    std::env::var("GRPC_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".to_string())
}

fn select_auth_config(
    platform_jwt_config: Option<&talon::control::config::proto::JwtIssuerConfig>,
) -> AuthConfig {
    if platform_jwt_config.is_some() {
        AuthConfig::jwt_platform()
    } else {
        AuthConfig::open()
    }
}

fn worker_session_concurrency() -> usize {
    std::env::var("TALON_WORKER_SESSION_CONCURRENCY")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1)
}

fn worker_handler(
    cp: Arc<ControlPlane>,
    config: Arc<Config>,
    scheduler_authenticator: Arc<SchedulerRequestAuthenticator>,
    worker_id: String,
    fanout_hub: Arc<FanoutHub>,
) -> WorkerEventHandler {
    let jwt_issuer = config
        .platform_auth
        .as_ref()
        .and_then(|auth| auth.jwt_issuer.as_ref())
        .map(|issuer| issuer.issuer.trim().to_string())
        .filter(|issuer| !issuer.is_empty());
    WorkerEventHandler {
        cp,
        config,
        mcp_registry: Arc::new(McpRegistry::new_with_jwt_issuer(jwt_issuer)),
        scheduler_authenticator,
        worker_id,
        fanout_hub,
        session_cancellations: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
    }
}

fn node_worker_socket_path(worker_id: &str) -> Result<PathBuf> {
    if let Ok(raw_url) = std::env::var("TALON_WORKER_ENDPOINT_URL") {
        let url = Url::parse(raw_url.trim()).context("invalid TALON_WORKER_ENDPOINT_URL")?;
        if url.scheme() == "unix" {
            let path = urlencoding::decode(url.path())
                .context("invalid unix worker endpoint path")?
                .into_owned();
            return Ok(PathBuf::from(path));
        }
    }
    if let Ok(path) = std::env::var("TALON_WORKER_UNIX_SOCKET_PATH") {
        let path = path.trim();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    Ok(std::env::temp_dir().join(format!("talon-node-worker-{worker_id}.sock")))
}

fn node_worker_endpoint(socket_path: &Path) -> resources_proto::WorkerEndpoint {
    resources_proto::WorkerEndpoint {
        url: format!("unix://{}", socket_path.display()),
        protocol: "grpc".to_string(),
        audience: std::env::var("TALON_WORKER_ENDPOINT_AUDIENCE").unwrap_or_default(),
    }
}

async fn prepare_worker_socket(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.with_context(|| {
            format!(
                "failed to create worker socket directory {}",
                parent.display()
            )
        })?;
    }
    match tokio::fs::metadata(path).await {
        Ok(metadata) if metadata.file_type().is_socket() => {
            tokio::fs::remove_file(path).await.with_context(|| {
                format!("failed to remove stale worker socket {}", path.display())
            })?;
        }
        Ok(_) => {
            anyhow::bail!(
                "refusing to replace non-socket worker endpoint path {}",
                path.display()
            );
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to inspect worker socket path {}", path.display())
            });
        }
    }
    Ok(())
}

async fn serve_worker_fanout_socket(
    socket_path: PathBuf,
    fanout_hub: Arc<FanoutHub>,
    shutdown: CancellationToken,
) -> Result<()> {
    prepare_worker_socket(&socket_path).await?;
    let listener = UnixListener::bind(&socket_path).with_context(|| {
        format!(
            "failed to bind worker fanout socket {}",
            socket_path.display()
        )
    })?;
    let incoming = UnixListenerStream::new(listener);
    let service =
        talon::gateway::rpc::worker_proto::fanout_service_server::FanoutServiceServer::new(
            talon::worker::fanout::FanoutServiceImpl::new(fanout_hub),
        );
    tracing::info!(
        endpoint = %format!("unix://{}", socket_path.display()),
        "Starting node worker fanout service"
    );
    let result = Server::builder()
        .add_service(service)
        .serve_with_incoming_shutdown(incoming, shutdown.cancelled_owned())
        .await
        .context("node worker fanout service failed");
    if let Err(err) = tokio::fs::remove_file(&socket_path).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %socket_path.display(), error = %err, "Failed to remove worker fanout socket");
        }
    }
    result
}

async fn spawn_subscription(
    pubsub: Arc<dyn MessagePublisher + Send + Sync>,
    handler: WorkerEventHandler,
    topic: &'static str,
    event_type: &'static str,
    concurrency: usize,
    shutdown: CancellationToken,
) -> Result<JoinHandle<Result<()>>> {
    let stream = pubsub.subscribe(topic).await?;
    Ok(tokio::spawn(async move {
        tracing::info!(
            topic,
            event_type,
            concurrency,
            "Starting node worker subscription"
        );
        stream
            .take_until(shutdown.cancelled_owned())
            .for_each_concurrent(concurrency, move |payload| {
                let handler = handler.clone();
                let span = tracing::info_span!(
                    "TalonNode.dispatch",
                    topic,
                    event_type,
                    "worker.session_concurrency" = concurrency,
                    payload_bytes = payload.len(),
                );
                async move {
                    if let Err(err) = handler.dispatch(Some(event_type), &payload).await {
                        tracing::error!(event_type, error = %err, "Colocated worker dispatch failed");
                    }
                }
                .instrument(span)
            })
            .await;
        Ok(())
    }))
}

async fn join_with_grace(task: &mut JoinHandle<Result<()>>) -> Result<()> {
    match tokio::time::timeout(std::time::Duration::from_secs(1), &mut *task).await {
        Ok(result) => match result {
            Ok(inner) => inner,
            Err(err) => Err(err.into()),
        },
        Err(_) => {
            task.abort();
            let _ = task.await;
            Ok(())
        }
    }
}

async fn join_unit_with_grace(task: &mut JoinHandle<()>) {
    if tokio::time::timeout(std::time::Duration::from_secs(1), &mut *task)
        .await
        .is_err()
    {
        task.abort();
        let _ = task.await;
    }
}

async fn run() -> Result<()> {
    let config = Arc::new(Config::load_default()?);
    if config
        .platform_auth
        .as_ref()
        .and_then(|auth| auth.jwt_issuer.as_ref())
        .is_some()
    {
        talon::control::security::platform_jwt::load_key()?;
    }
    let cp = Arc::new(build_control_plane(&config).await?);
    let scheduler_authenticator =
        Arc::new(SchedulerRequestAuthenticator::from_config(&config).await?);
    let worker_id = talon::worker::registration::worker_id();
    let fanout_hub = Arc::new(FanoutHub::new());
    let handler = worker_handler(
        Arc::clone(&cp),
        Arc::clone(&config),
        scheduler_authenticator,
        worker_id.clone(),
        fanout_hub.clone(),
    );
    let shutdown = CancellationToken::new();
    let session_concurrency = worker_session_concurrency();
    let worker_socket_path = node_worker_socket_path(&worker_id)?;
    let worker_endpoint = node_worker_endpoint(&worker_socket_path);
    let worker_registration =
        talon::worker::registration::WorkerRegistration::new(&worker_id, env!("CARGO_PKG_VERSION"))
            .with_endpoints(vec![worker_endpoint]);
    let mut heartbeat_task = tokio::spawn(talon::worker::registration::run_worker_heartbeat(
        Arc::clone(&cp),
        worker_registration,
        shutdown.child_token(),
    ));
    let mut fanout_task = tokio::spawn(serve_worker_fanout_socket(
        worker_socket_path,
        fanout_hub,
        shutdown.child_token(),
    ));

    let mut subscription_tasks = vec![
        spawn_subscription(
            Arc::clone(&cp.pubsub),
            handler.clone(),
            topics::SESSION_DISPATCH_TOPIC,
            "session_dispatch",
            session_concurrency,
            shutdown.child_token(),
        )
        .await?,
        spawn_subscription(
            Arc::clone(&cp.pubsub),
            handler.clone(),
            topics::RESOURCE_LIFECYCLE_TOPIC,
            "resource_lifecycle",
            1,
            shutdown.child_token(),
        )
        .await?,
        spawn_subscription(
            Arc::clone(&cp.pubsub),
            handler.clone(),
            topics::SESSION_CONTROL_TOPIC,
            "session_control",
            1,
            shutdown.child_token(),
        )
        .await?,
        spawn_subscription(
            Arc::clone(&cp.pubsub),
            handler,
            topics::INDEX_EVENTS_TOPIC,
            "index",
            1,
            shutdown.child_token(),
        )
        .await?,
    ];
    let platform_jwt_config = config
        .platform_auth
        .as_ref()
        .and_then(|auth| auth.jwt_issuer.clone());
    let mut auth_config = select_auth_config(platform_jwt_config.as_ref());
    if auth_config.mode == talon::gateway::auth::AuthMode::Jwt {
        auth_config.platform_jwt_issuer = platform_jwt_config
            .as_ref()
            .map(|config| config.issuer.trim().to_string())
            .filter(|issuer| !issuer.is_empty());
    }
    let gateway = Gateway::new_with_trust_and_platform_jwt(
        Some(auth_config),
        config.trust.clone(),
        platform_jwt_config,
        Arc::clone(&cp.kv),
        Arc::clone(&cp.pubsub),
        Arc::clone(&cp.scheduler),
        Arc::clone(&cp.objects),
        Arc::clone(&cp.documents),
    );
    let rpc_addr = gateway_addr();
    let mut rpc_task = tokio::spawn({
        let shutdown = shutdown.child_token();
        async move {
            gateway
                .start_rpc_server_with_shutdown(&rpc_addr, shutdown.cancelled_owned())
                .await
        }
    });

    enum Exit {
        Rpc(Result<()>),
        Fanout(Result<()>),
        Shutdown,
    }

    let exit = tokio::select! {
        result = &mut rpc_task => Exit::Rpc(match result {
            Ok(inner) => inner,
            Err(err) => Err(err.into()),
        }),
        result = &mut fanout_task => Exit::Fanout(match result {
            Ok(inner) => inner,
            Err(err) => Err(err.into()),
        }),
        result = signal::ctrl_c() => match result {
            Ok(()) => Exit::Shutdown,
            Err(_) => Exit::Shutdown,
        },
    };

    shutdown.cancel();
    let result = match exit {
        Exit::Rpc(result) => result,
        Exit::Fanout(result) => result,
        Exit::Shutdown => join_with_grace(&mut rpc_task).await,
    };
    let _ = join_with_grace(&mut fanout_task).await;
    join_unit_with_grace(&mut heartbeat_task).await;
    for task in &mut subscription_tasks {
        let _ = join_with_grace(task).await;
    }
    result
}

#[tokio::main]
async fn main() -> Result<()> {
    talon::control::security::install_jwt_crypto_provider();
    let _telemetry_guard = talon::control::telemetry::init_from_env("talon-node")?;
    talon::control::profiling::init_cpu_profiler_from_env(|name| std::env::var(name).ok())?;
    talon::control::profiling::init_heap_profiler_from_env(|name| std::env::var(name).ok())?;
    tracing::info!("Starting Talon node runtime...");
    run().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use talon::control::{keys, ns, ProtoKeyValueStoreExt};
    use talon::test_support::{EmptyPubSub, MockKvStore};
    use tempfile::tempdir;

    #[test]
    fn node_worker_endpoint_uses_unix_uri() {
        let endpoint = node_worker_endpoint(Path::new("/tmp/talon-node-worker.sock"));
        assert_eq!(endpoint.url, "unix:///tmp/talon-node-worker.sock");
        assert_eq!(endpoint.protocol, "grpc");
    }

    #[tokio::test]
    async fn node_worker_fanout_registration_publishes_unix_endpoint() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("fanout.sock");
        let endpoint = node_worker_endpoint(&socket_path);
        let cp =
            ControlPlane::builder(Arc::new(MockKvStore::default()), Arc::new(EmptyPubSub)).build();
        let registration =
            talon::worker::registration::WorkerRegistration::new("node-worker", "1.2.3")
                .with_endpoints(vec![endpoint.clone()]);

        talon::worker::registration::upsert_worker(&cp, &registration)
            .await
            .unwrap();
        talon::worker::registration::patch_worker_status(&cp, &registration, "ready")
            .await
            .unwrap();

        let worker = cp
            .kv
            .get_msg::<resources_proto::Worker>(&keys::ResourceKey::new(
                ns::TALON_SYSTEM,
                &[],
                "Worker",
                "node-worker",
            ))
            .await
            .unwrap()
            .unwrap();
        let endpoints = worker.status.unwrap().endpoints;
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].url, endpoint.url);
        assert_eq!(endpoints[0].protocol, "grpc");
    }
}
