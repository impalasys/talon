// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use std::sync::Arc;
use talon::config::{Config, ConfigExt};
use talon::control::build_control_plane;
use talon::control::ControlPlane;
use talon::gateway::auth::AuthConfig;
use talon::gateway::server::Gateway;
use tokio::signal;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

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

fn gateway_addresses<F>(mut get: F) -> (String, String)
where
    F: FnMut(&str) -> Option<String>,
{
    (
        get("GRPC_ADDR").unwrap_or_else(|| "0.0.0.0:50051".to_string()),
        get("GATEWAY_UI_ADDR").unwrap_or_else(|| "0.0.0.0:50052".to_string()),
    )
}

fn build_gateway(auth_config: AuthConfig, cp: ControlPlane) -> Gateway {
    Gateway::new(Some(auth_config), cp.kv, cp.pubsub, cp.scheduler)
}

fn spawn_gateway_tasks(
    gateway: Gateway,
    rpc_addr: String,
    ui_addr: String,
    shutdown_token: CancellationToken,
) -> (JoinHandle<Result<()>>, JoinHandle<Result<()>>) {
    let rpc_gateway = gateway.clone();
    let ui_gateway = gateway;
    let rpc_shutdown = shutdown_token.child_token();
    let ui_shutdown = shutdown_token.child_token();
    let rpc_task = tokio::spawn(async move {
        rpc_gateway
            .start_rpc_server_with_shutdown(&rpc_addr, rpc_shutdown.cancelled_owned())
            .await
    });
    let ui_task = tokio::spawn(async move {
        ui_gateway
            .start_http_ui_server_with_shutdown(&ui_addr, ui_shutdown.cancelled_owned())
            .await
    });
    (rpc_task, ui_task)
}

async fn run_gateway_with<FGetAuth, FGetAddr, FShutdown>(
    cp: ControlPlane,
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
    let gateway = build_gateway(auth_config, cp);
    let (rpc_addr, ui_addr) = gateway_addresses(addr_get);
    let shutdown_token = CancellationToken::new();
    let (rpc_task, ui_task) =
        spawn_gateway_tasks(gateway, rpc_addr, ui_addr, shutdown_token.child_token());
    wait_for_server_tasks(rpc_task, ui_task, shutdown_token, shutdown).await
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
    let cp = build_cp(&config).await?;
    run_gateway_with(cp, auth_get, addr_get, shutdown).await
}

async fn wait_for_server_tasks<F>(
    rpc_task: JoinHandle<Result<()>>,
    ui_task: JoinHandle<Result<()>>,
    shutdown_token: CancellationToken,
    shutdown: F,
) -> Result<()>
where
    F: std::future::Future,
{
    fn combine_task_results(
        primary: Result<()>,
        sibling: Result<()>,
        sibling_name: &str,
    ) -> Result<()> {
        match (primary, sibling) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(primary), Ok(())) => Err(primary),
            (Ok(()), Err(sibling)) => Err(sibling),
            (Err(primary), Err(sibling)) => Err(anyhow::anyhow!(
                "{}; {} task also failed: {}",
                primary,
                sibling_name,
                sibling
            )),
        }
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

    let mut rpc_task = rpc_task;
    let mut ui_task = ui_task;
    tokio::pin!(shutdown);

    enum Exit {
        Rpc(Result<()>),
        Ui(Result<()>),
        Shutdown,
    }

    let result = tokio::select! {
        res = &mut rpc_task => Exit::Rpc(match res {
            Ok(inner) => inner,
            Err(err) => Err(err.into()),
        }),
        res = &mut ui_task => Exit::Ui(match res {
            Ok(inner) => inner,
            Err(err) => Err(err.into()),
        }),
        _ = &mut shutdown => {
            tracing::info!("Shutting down...");
            Exit::Shutdown
        }
    };
    shutdown_token.cancel();

    match result {
        Exit::Rpc(result) => {
            let ui_result = join_with_grace(&mut ui_task).await;
            combine_task_results(result, ui_result, "ui")
        }
        Exit::Ui(result) => {
            let rpc_result = join_with_grace(&mut rpc_task).await;
            combine_task_results(result, rpc_result, "rpc")
        }
        Exit::Shutdown => {
            let rpc_result = join_with_grace(&mut rpc_task).await;
            let ui_result = join_with_grace(&mut ui_task).await;
            combine_task_results(rpc_result, ui_result, "ui")
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    talon::security::install_jwt_crypto_provider();
    tracing_subscriber::fmt::init();
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

#[cfg(test)]
mod tests {
    use super::{
        build_gateway, gateway_addresses, run_gateway_with, run_server_main_with,
        select_auth_config, spawn_gateway_tasks, wait_for_server_tasks,
    };
    use futures::StreamExt;
    use std::sync::Arc;
    use talon::config::Config;
    use talon::control::{
        scheduler::NoopSchedulerBackend, ControlPlane, KeyValueStore, MessagePublisher,
    };
    use talon::gateway::auth::AuthConfig;
    use talon::gateway::auth::AuthMode;
    use talon::test_support::{EmptyPubSub, MockKvStore};
    use tokio::sync::oneshot;
    use tokio_util::sync::CancellationToken;

    fn test_control_plane() -> ControlPlane {
        ControlPlane {
            kv: Arc::new(MockKvStore::default()),
            pubsub: Arc::new(EmptyPubSub),
            scheduler: Arc::new(NoopSchedulerBackend),
        }
    }

    #[test]
    fn select_auth_config_prefers_jwt_then_token_then_password() {
        let jwt = select_auth_config(|name| match name {
            "GATEWAY_JWT_SECRET" => Some("jwt-secret".to_string()),
            "GATEWAY_TOKEN" => Some("token".to_string()),
            "GATEWAY_PASSWORD" => Some("password".to_string()),
            _ => None,
        });
        assert_eq!(jwt.mode, AuthMode::Jwt);
        assert_eq!(jwt.jwt_secret.as_deref(), Some("jwt-secret"));

        let token = select_auth_config(|name| match name {
            "GATEWAY_TOKEN" => Some("token".to_string()),
            "GATEWAY_PASSWORD" => Some("password".to_string()),
            _ => None,
        });
        assert_eq!(token.mode, AuthMode::Token);
        assert_eq!(token.tokens, vec!["token".to_string()]);

        let password = select_auth_config(|name| match name {
            "GATEWAY_PASSWORD" => Some("password".to_string()),
            _ => None,
        });
        assert_eq!(password.mode, AuthMode::Password);
        assert_eq!(password.password.as_deref(), Some("password"));
    }

    #[test]
    fn select_auth_config_defaults_to_open() {
        let auth = select_auth_config(|_| None);
        assert_eq!(auth.mode, AuthMode::Open);
        assert!(auth.password.is_none());
        assert!(auth.jwt_secret.is_none());
        assert!(auth.tokens.is_empty());
    }

    #[test]
    fn gateway_addresses_use_env_or_defaults() {
        let defaults = gateway_addresses(|_| None);
        assert_eq!(defaults.0, "0.0.0.0:50051");
        assert_eq!(defaults.1, "0.0.0.0:50052");

        let custom = gateway_addresses(|name| match name {
            "GRPC_ADDR" => Some("127.0.0.1:6001".to_string()),
            "GATEWAY_UI_ADDR" => Some("127.0.0.1:6002".to_string()),
            _ => None,
        });
        assert_eq!(custom.0, "127.0.0.1:6001");
        assert_eq!(custom.1, "127.0.0.1:6002");
    }

    #[test]
    fn build_gateway_preserves_auth_and_dependencies() {
        let auth = AuthConfig::tokens(vec!["gateway-token".to_string()]);
        let gateway = build_gateway(auth, test_control_plane());
        assert_eq!(
            gateway.auth_config.as_ref().map(|cfg| cfg.mode.clone()),
            Some(AuthMode::Token)
        );
    }

    #[tokio::test]
    async fn mock_control_plane_helpers_cover_storage_and_pubsub_branches() {
        let kv = MockKvStore::default();
        assert_eq!(kv.get("root/missing").await.unwrap(), None);

        kv.set("root/agents/a", b"one").await.unwrap();
        kv.set("root/agents/b", b"two").await.unwrap();
        kv.set("other/agents/c", b"three").await.unwrap();
        assert_eq!(
            kv.get("root/agents/a").await.unwrap(),
            Some(b"one".to_vec())
        );

        assert!(kv
            .compare_and_swap("root/agents/new", None, b"created")
            .await
            .unwrap());
        assert!(kv
            .compare_and_swap("root/agents/a", Some(b"one"), b"updated")
            .await
            .unwrap());
        assert!(!kv
            .compare_and_swap("root/agents/a", Some(b"wrong"), b"nope")
            .await
            .unwrap());

        let keys = kv.list_keys("root/agents/").await.unwrap();
        assert_eq!(
            keys,
            vec![
                "root/agents/a".to_string(),
                "root/agents/b".to_string(),
                "root/agents/new".to_string(),
            ]
        );

        kv.delete("root/agents/b").await.unwrap();
        assert_eq!(kv.get("root/agents/b").await.unwrap(), None);

        let pubsub = EmptyPubSub;
        pubsub.publish("topic", b"payload").await.unwrap();
        let items = pubsub
            .subscribe("topic")
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn wait_for_server_tasks_returns_rpc_and_ui_results() {
        let shutdown_token = CancellationToken::new();
        let rpc_ok = tokio::spawn(async { Ok(()) });
        let ui_pending = tokio::spawn(async {
            futures::future::pending::<()>().await;
            #[allow(unreachable_code)]
            Ok(())
        });
        wait_for_server_tasks(
            rpc_ok,
            ui_pending,
            shutdown_token.child_token(),
            futures::future::pending::<()>(),
        )
        .await
        .unwrap();

        let shutdown_token = CancellationToken::new();
        let rpc_pending = tokio::spawn(async {
            futures::future::pending::<()>().await;
            #[allow(unreachable_code)]
            Ok(())
        });
        let ui_err = tokio::spawn(async { anyhow::bail!("ui failed") });
        let err = wait_for_server_tasks(
            rpc_pending,
            ui_err,
            shutdown_token.child_token(),
            futures::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("ui failed"));

        let shutdown_token = CancellationToken::new();
        let rpc_err = tokio::spawn(async { anyhow::bail!("rpc failed") });
        let ui_pending = tokio::spawn(async {
            futures::future::pending::<()>().await;
            #[allow(unreachable_code)]
            Ok(())
        });
        let err = wait_for_server_tasks(
            rpc_err,
            ui_pending,
            shutdown_token.child_token(),
            futures::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("rpc failed"));

        let shutdown_token = CancellationToken::new();
        let rpc_err = tokio::spawn(async { anyhow::bail!("rpc failed") });
        let ui_err = tokio::spawn(async { anyhow::bail!("ui failed") });
        let err = wait_for_server_tasks(
            rpc_err,
            ui_err,
            shutdown_token.child_token(),
            futures::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        let rendered = err.to_string();
        assert!(rendered.contains("rpc failed"));
        assert!(rendered.contains("ui failed"));
        assert!(
            rendered.contains("ui task also failed") || rendered.contains("rpc task also failed")
        );
    }

    #[tokio::test]
    async fn wait_for_server_tasks_returns_join_errors_and_shutdown() {
        let shutdown_token = CancellationToken::new();
        let rpc_panic = tokio::spawn(async {
            panic!("rpc panic");
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        });
        let ui_pending = tokio::spawn(async {
            futures::future::pending::<()>().await;
            #[allow(unreachable_code)]
            Ok(())
        });
        let err = wait_for_server_tasks(
            rpc_panic,
            ui_pending,
            shutdown_token.child_token(),
            futures::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("rpc panic"));

        let shutdown_token = CancellationToken::new();
        let rpc_shutdown = shutdown_token.child_token();
        let ui_shutdown = shutdown_token.child_token();
        let rpc_pending = tokio::spawn(async move {
            rpc_shutdown.cancelled().await;
            Ok(())
        });
        let ui_pending = tokio::spawn(async move {
            ui_shutdown.cancelled().await;
            Ok(())
        });
        let (tx, rx) = oneshot::channel::<()>();
        let shutdown_task = tokio::spawn(async move {
            let _ = tx.send(());
        });
        wait_for_server_tasks(rpc_pending, ui_pending, shutdown_token, async move {
            let _ = rx.await;
        })
        .await
        .unwrap();
        shutdown_task.await.unwrap();

        let shutdown_token = CancellationToken::new();
        let rpc_shutdown = shutdown_token.child_token();
        let rpc_pending = tokio::spawn(async move {
            rpc_shutdown.cancelled().await;
            Ok(())
        });
        let ui_hung = tokio::spawn(async {
            futures::future::pending::<()>().await;
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        });
        let (tx, rx) = oneshot::channel::<()>();
        let shutdown_task = tokio::spawn(async move {
            let _ = tx.send(());
        });
        wait_for_server_tasks(rpc_pending, ui_hung, shutdown_token, async move {
            let _ = rx.await;
        })
        .await
        .unwrap();
        shutdown_task.await.unwrap();
    }

    #[tokio::test]
    async fn spawn_gateway_tasks_binds_valid_listeners() {
        let gateway = build_gateway(AuthConfig::open(), test_control_plane());
        let rpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("rpc probe should bind");
        let rpc_addr = rpc_listener.local_addr().expect("rpc addr");
        drop(rpc_listener);
        let ui_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ui probe should bind");
        let ui_addr = ui_listener.local_addr().expect("ui addr");
        drop(ui_listener);

        let (rpc_task, ui_task) = spawn_gateway_tasks(
            gateway,
            rpc_addr.to_string(),
            ui_addr.to_string(),
            CancellationToken::new(),
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        rpc_task.abort();
        ui_task.abort();

        let rpc_err = rpc_task.await.expect_err("rpc task should be aborted");
        let ui_err = ui_task.await.expect_err("ui task should be aborted");
        assert!(rpc_err.is_cancelled());
        assert!(ui_err.is_cancelled());
    }

    #[tokio::test]
    async fn run_gateway_with_surfaces_startup_errors() {
        let err = run_gateway_with(
            test_control_plane(),
            |name| match name {
                "GATEWAY_TOKEN" => Some("token".to_string()),
                _ => None,
            },
            |name| match name {
                "GRPC_ADDR" => Some("127.0.0.1:0".to_string()),
                "GATEWAY_UI_ADDR" => Some("not-an-addr".to_string()),
                _ => None,
            },
            futures::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        let text = err.to_string();
        assert!(
            text.contains("not-an-addr")
                || text.contains("invalid")
                || text.contains("failed to lookup address information")
        );
    }

    #[tokio::test]
    async fn run_gateway_with_can_start_and_shutdown_cleanly() {
        let rpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("rpc probe should bind");
        let rpc_addr = rpc_listener.local_addr().expect("rpc addr");
        drop(rpc_listener);

        let ui_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ui probe should bind");
        let ui_addr = ui_listener.local_addr().expect("ui addr");
        drop(ui_listener);

        let (tx, rx) = oneshot::channel::<()>();
        let task = tokio::spawn(run_gateway_with(
            test_control_plane(),
            |_| None,
            move |name| match name {
                "GRPC_ADDR" => Some(rpc_addr.to_string()),
                "GATEWAY_UI_ADDR" => Some(ui_addr.to_string()),
                _ => None,
            },
            async move {
                let _ = rx.await;
            },
        ));

        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
        let _ = tx.send(());
        task.await
            .expect("task should join")
            .expect("shutdown should succeed");
    }

    #[tokio::test]
    async fn run_server_main_with_surfaces_config_and_control_plane_errors() {
        let config_err = run_server_main_with(
            || anyhow::bail!("config failed"),
            |_| async { Ok(test_control_plane()) },
            |_| None,
            |_| None,
            futures::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        assert!(config_err.to_string().contains("config failed"));

        let cp_err = run_server_main_with(
            || Ok(Arc::new(Config::default())),
            |_| async { anyhow::bail!("control plane failed") },
            |_| None,
            |_| None,
            futures::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        assert!(cp_err.to_string().contains("control plane failed"));
    }

    #[tokio::test]
    async fn run_server_main_with_starts_and_shuts_down() {
        let rpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("rpc probe should bind");
        let rpc_addr = rpc_listener.local_addr().expect("rpc addr");
        drop(rpc_listener);

        let ui_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ui probe should bind");
        let ui_addr = ui_listener.local_addr().expect("ui addr");
        drop(ui_listener);

        let (tx, rx) = oneshot::channel::<()>();
        let task = tokio::spawn(run_server_main_with(
            || Ok(Arc::new(Config::default())),
            |_| async { Ok(test_control_plane()) },
            |_| None,
            move |name| match name {
                "GRPC_ADDR" => Some(rpc_addr.to_string()),
                "GATEWAY_UI_ADDR" => Some(ui_addr.to_string()),
                _ => None,
            },
            async move {
                let _ = rx.await;
            },
        ));

        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
        let _ = tx.send(());
        task.await
            .expect("task should join")
            .expect("shutdown should succeed");
    }
}
