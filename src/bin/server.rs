// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use std::sync::Arc;
use talon::control::build_control_plane;
use talon::control::config::proto::{JwtIssuerConfig, TrustConfig};
use talon::control::config::{Config, ConfigExt};
use talon::control::security::platform_jwt;
use talon::control::ControlPlane;
use talon::gateway::auth::{AuthConfig, AuthMode};
use talon::gateway::server::Gateway;
use tokio::signal;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[cfg(feature = "heap-profile")]
#[global_allocator]
static GLOBAL_ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn select_auth_config(platform_jwt_config: Option<&JwtIssuerConfig>) -> AuthConfig {
    if platform_jwt_config.is_some() {
        AuthConfig::jwt_platform()
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
    mut auth_config: AuthConfig,
    trust_config: Option<TrustConfig>,
    platform_jwt_config: Option<JwtIssuerConfig>,
    cp: ControlPlane,
) -> Gateway {
    if auth_config.mode == AuthMode::Jwt {
        auth_config.platform_jwt_issuer = platform_jwt_config
            .as_ref()
            .map(|config| config.issuer.trim().to_string())
            .filter(|issuer| !issuer.is_empty());
    }
    Gateway::new_with_trust_and_platform_jwt(
        Some(auth_config),
        trust_config,
        platform_jwt_config,
        cp.kv,
        cp.pubsub,
        cp.scheduler,
        cp.objects,
        cp.documents,
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

async fn run_gateway_with<FGetAddr, FShutdown>(
    cp: ControlPlane,
    trust_config: Option<TrustConfig>,
    platform_jwt_config: Option<JwtIssuerConfig>,
    addr_get: FGetAddr,
    shutdown: FShutdown,
) -> Result<()>
where
    FGetAddr: FnMut(&str) -> Option<String>,
    FShutdown: std::future::Future,
{
    let auth_config = select_auth_config(platform_jwt_config.as_ref());
    let gateway = build_gateway(auth_config, trust_config, platform_jwt_config, cp);
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

async fn run_server_main_with<FLoad, FBuild, FBuildFuture, FGetAddr, FShutdown>(
    load_config: FLoad,
    build_cp: FBuild,
    addr_get: FGetAddr,
    shutdown: FShutdown,
) -> Result<()>
where
    FLoad: FnOnce() -> Result<Arc<Config>>,
    FBuild: FnOnce(&Arc<Config>) -> FBuildFuture,
    FBuildFuture: std::future::Future<Output = Result<ControlPlane>>,
    FGetAddr: FnMut(&str) -> Option<String>,
    FShutdown: std::future::Future,
{
    let config = load_config()?;
    let trust_config = config.trust.clone();
    let platform_jwt_config = config
        .platform_auth
        .as_ref()
        .and_then(|auth| auth.jwt_issuer.clone());
    if platform_jwt_config.is_some() {
        platform_jwt::load_key()?;
    }
    let cp = build_cp(&config).await?;
    run_gateway_with(cp, trust_config, platform_jwt_config, addr_get, shutdown).await
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
        signal::ctrl_c(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use talon::test_support::{EmptyPubSub, MockKvStore};

    fn test_control_plane() -> ControlPlane {
        ControlPlane::builder(Arc::new(MockKvStore::default()), Arc::new(EmptyPubSub)).build()
    }

    #[test]
    fn build_gateway_uses_platform_jwt_when_issuer_is_configured() {
        let platform_config = JwtIssuerConfig {
            issuer: "https://talon.example.com".to_string(),
        };
        let gateway = build_gateway(
            select_auth_config(Some(&platform_config)),
            None,
            Some(platform_config),
            test_control_plane(),
        );
        let auth_config = gateway.auth_config.as_ref().unwrap();

        assert_eq!(auth_config.mode, AuthMode::Jwt);
        assert_eq!(
            auth_config.platform_jwt_issuer.as_deref(),
            Some("https://talon.example.com")
        );
    }
}
