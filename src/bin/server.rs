// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use std::sync::Arc;
use talon::control::build_control_plane;
use talon::control::config::proto::TrustConfig;
use talon::control::config::{Config, ConfigExt};
use talon::control::security::platform_jwt;
use talon::control::ControlPlane;
use talon::gateway::auth::AuthConfig;
use talon::gateway::server::Gateway;
use tokio::signal;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[cfg(feature = "heap-profile")]
#[global_allocator]
static GLOBAL_ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn select_auth_config() -> AuthConfig {
    AuthConfig::jwt_platform()
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
    addr_get: FGetAddr,
    shutdown: FShutdown,
) -> Result<()>
where
    FGetAddr: FnMut(&str) -> Option<String>,
    FShutdown: std::future::Future,
{
    let auth_config = select_auth_config();
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
    platform_jwt::load_key()?;
    platform_jwt::issuer()?;
    let cp = build_cp(&config).await?;
    run_gateway_with(cp, trust_config, addr_get, shutdown).await
}

#[tokio::main]
async fn main() -> Result<()> {
    talon::control::security::install_jwt_crypto_provider();
    let _telemetry_guard = talon::control::telemetry::init_from_env("talon-server")?;
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

    const TEST_RSA_PRIVATE_KEY: &str = include_str!("../control/security/test_rsa_private_key.pem");

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

    fn test_control_plane() -> ControlPlane {
        ControlPlane::builder(Arc::new(MockKvStore::default()), Arc::new(EmptyPubSub)).build()
    }

    #[test]
    fn build_gateway_uses_platform_jwt_when_private_key_is_configured() {
        let _env_lock = talon::test_support::env_lock();
        let _private_key = EnvGuard::set(
            platform_jwt::TALON_JWT_PRIVATE_KEY_PEM_ENV,
            TEST_RSA_PRIVATE_KEY,
        );

        let gateway = build_gateway(select_auth_config(), None, test_control_plane());
        let auth_config = gateway.auth_config.as_ref().unwrap();

        assert_eq!(auth_config.mode, talon::gateway::auth::AuthMode::Jwt);
    }
}
