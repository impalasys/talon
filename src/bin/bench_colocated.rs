// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use futures::StreamExt;
use std::sync::Arc;
use talon::{
    config::{Config, ConfigExt},
    control::{build_control_plane, topics, ControlPlane, MessagePublisher},
    gateway::{auth::AuthConfig, server::Gateway},
    worker::{
        mcp_registry::McpRegistry, scheduler_auth::SchedulerRequestAuthenticator,
        WorkerEventHandler,
    },
};
use tokio::{signal, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

#[cfg(feature = "heap-profile")]
#[global_allocator]
static GLOBAL_ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn gateway_addresses() -> (String, String) {
    (
        std::env::var("GRPC_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".to_string()),
        std::env::var("GATEWAY_UI_ADDR").unwrap_or_else(|_| "0.0.0.0:50052".to_string()),
    )
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
) -> WorkerEventHandler {
    WorkerEventHandler {
        cp,
        config,
        mcp_registry: Arc::new(McpRegistry::new()),
        scheduler_authenticator,
        session_cancellations: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
    }
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
            "Starting colocated benchmark worker subscription"
        );
        stream
            .take_until(shutdown.cancelled_owned())
            .for_each_concurrent(concurrency, move |payload| {
                let handler = handler.clone();
                let span = tracing::info_span!(
                    "BenchColocated.dispatch",
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

async fn run() -> Result<()> {
    let config = Arc::new(Config::load_default()?);
    let cp = Arc::new(build_control_plane(&config).await?);
    let scheduler_authenticator =
        Arc::new(SchedulerRequestAuthenticator::from_config(&config).await?);
    let handler = worker_handler(
        Arc::clone(&cp),
        Arc::clone(&config),
        scheduler_authenticator,
    );
    let shutdown = CancellationToken::new();
    let session_concurrency = worker_session_concurrency();

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
            handler,
            topics::SESSION_CONTROL_TOPIC,
            "session_control",
            1,
            shutdown.child_token(),
        )
        .await?,
    ];

    let gateway = Gateway::new(
        Some(AuthConfig::open()),
        Arc::clone(&cp.kv),
        Arc::clone(&cp.pubsub),
        Arc::clone(&cp.scheduler),
    );
    let (rpc_addr, ui_addr) = gateway_addresses();
    let rpc_gateway = gateway.clone();
    let ui_gateway = gateway;
    let mut rpc_task = tokio::spawn({
        let shutdown = shutdown.child_token();
        async move {
            rpc_gateway
                .start_rpc_server_with_shutdown(&rpc_addr, shutdown.cancelled_owned())
                .await
        }
    });
    let mut ui_task = tokio::spawn({
        let shutdown = shutdown.child_token();
        async move {
            ui_gateway
                .start_http_ui_server_with_shutdown(&ui_addr, shutdown.cancelled_owned())
                .await
        }
    });

    enum Exit {
        Rpc(Result<()>),
        Ui(Result<()>),
        Shutdown,
    }

    let exit = tokio::select! {
        result = &mut rpc_task => Exit::Rpc(match result {
            Ok(inner) => inner,
            Err(err) => Err(err.into()),
        }),
        result = &mut ui_task => Exit::Ui(match result {
            Ok(inner) => inner,
            Err(err) => Err(err.into()),
        }),
        result = signal::ctrl_c() => match result {
            Ok(()) => Exit::Shutdown,
            Err(_) => Exit::Shutdown,
        },
    };

    shutdown.cancel();
    let rpc_result = join_with_grace(&mut rpc_task).await;
    let ui_result = join_with_grace(&mut ui_task).await;
    for task in &mut subscription_tasks {
        let _ = join_with_grace(task).await;
    }

    match exit {
        Exit::Rpc(result) | Exit::Ui(result) => {
            result?;
            rpc_result?;
            ui_result
        }
        Exit::Shutdown => {
            rpc_result?;
            ui_result
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    talon::security::install_jwt_crypto_provider();
    let _telemetry_guard = talon::telemetry::init_from_env("talon-worker")?;
    talon::profiling::init_cpu_profiler_from_env(|name| std::env::var(name).ok())?;
    talon::profiling::init_heap_profiler_from_env(|name| std::env::var(name).ok())?;
    tracing::info!("Starting colocated Talon benchmark runtime...");
    run().await
}
