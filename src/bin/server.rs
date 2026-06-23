// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use std::sync::Arc;
use talon::control::build_control_plane;
use talon::control::config::proto::TrustConfig;
use talon::control::config::{Config, ConfigExt};
use talon::control::ControlPlane;
use talon::gateway::auth::AuthConfig;
use talon::gateway::server::Gateway;
use tokio::signal;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[cfg(feature = "heap-profile")]
#[global_allocator]
static GLOBAL_ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn select_auth_config<F>(mut get: F) -> AuthConfig
where
    F: FnMut(&str) -> Option<String>,
{
    if let Some(secret) = get("GATEWAY_JWT_SECRET") {
        AuthConfig::jwt(secret)
    } else if let Some(token) = get("GATEWAY_TOKEN") {
        AuthConfig::tokens(vec![token])
    } else if let Some(password) = get("GATEWAY_PASSWORD") {
        AuthConfig::password(password)
    } else {
        AuthConfig::open()
    }
}

fn gateway_addr<F>(mut get: F) -> String
where
    F: FnMut(&str) -> Option<String>,
{
    get("GRPC_ADDR").unwrap_or_else(|| "0.0.0.0:50051".to_string())
}

fn build_gateway(
    auth_config: AuthConfig,
    trust_config: Option<TrustConfig>,
    cp: ControlPlane,
) -> Gateway {
    Gateway::new_with_trust(
        Some(auth_config),
        trust_config,
        cp.kv,
        cp.pubsub,
        cp.scheduler,
        cp.objects,
    )
}

fn spawn_gateway_task(
    gateway: Gateway,
    rpc_addr: String,
    shutdown_token: CancellationToken,
) -> JoinHandle<Result<()>> {
    tokio::spawn(async move {
        gateway
            .start_rpc_server_with_shutdown(&rpc_addr, shutdown_token.cancelled_owned())
            .await
    })
}

async fn run_gateway_with<FGetAuth, FGetAddr, FShutdown>(
    cp: ControlPlane,
    trust_config: Option<TrustConfig>,
    auth_get: FGetAuth,
    addr_get: FGetAddr,
    shutdown: FShutdown,
) -> Result<()>
where
    FGetAuth: FnMut(&str) -> Option<String>,
    FGetAddr: FnMut(&str) -> Option<String>,
    FShutdown: std::future::Future,
{
    let auth_config = select_auth_config(auth_get);
    let gateway = build_gateway(auth_config, trust_config, cp);
    let rpc_addr = gateway_addr(addr_get);
    let shutdown_token = CancellationToken::new();
    let mut task = spawn_gateway_task(gateway, rpc_addr, shutdown_token.child_token());
    tokio::pin!(shutdown);

    let result = tokio::select! {
        res = &mut task => match res {
            Ok(inner) => inner,
            Err(err) => Err(err.into()),
        },
        _ = &mut shutdown => {
            tracing::info!("Shutting down...");
            shutdown_token.cancel();
            match tokio::time::timeout(std::time::Duration::from_secs(1), &mut task).await {
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
    };
    shutdown_token.cancel();
    result
}

async fn run_server_main_with<FLoad, FBuild, FBuildFuture, FGetAuth, FGetAddr, FShutdown>(
    load_config: FLoad,
    build_cp: FBuild,
    auth_get: FGetAuth,
    addr_get: FGetAddr,
    shutdown: FShutdown,
) -> Result<()>
where
    FLoad: FnOnce() -> Result<Arc<Config>>,
    FBuild: FnOnce(&Arc<Config>) -> FBuildFuture,
    FBuildFuture: std::future::Future<Output = Result<ControlPlane>>,
    FGetAuth: FnMut(&str) -> Option<String>,
    FGetAddr: FnMut(&str) -> Option<String>,
    FShutdown: std::future::Future,
{
    let config = load_config()?;
    let trust_config = config.trust.clone();
    let cp = build_cp(&config).await?;
    run_gateway_with(cp, trust_config, auth_get, addr_get, shutdown).await
}

#[tokio::main]
async fn main() -> Result<()> {
    talon::control::security::install_jwt_crypto_provider();
    tracing_subscriber::fmt::init();
    talon::control::profiling::init_heap_profiler_from_env(|name| std::env::var(name).ok())?;
    tracing::info!("Starting Talon Gateway Server...");
    run_server_main_with(
        || Ok(Arc::new(Config::load_default()?)),
        |config| {
            let config = Arc::clone(config);
            async move { build_control_plane(&config).await }
        },
        |name| std::env::var(name).ok(),
        |name| std::env::var(name).ok(),
        signal::ctrl_c(),
    )
    .await
}
