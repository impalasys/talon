// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use axum::{
    body::Bytes, extract::State, http::HeaderMap, response::IntoResponse, routing::post, Json,
    Router,
};
use serde::Deserialize;
use std::sync::Arc;
use talon::config::{Config, ConfigExt};
use talon::control::build_control_plane;
use talon::control::pubsub::{fully_qualified_subscription_name, fully_qualified_topic_name};
use talon::control::topics;
use talon::control::ControlPlane;
use talon::worker::{scheduler_auth::SchedulerRequestAuthenticator, WorkerEventHandler};
use tokio::signal;
use tokio_util::sync::CancellationToken;

#[async_trait::async_trait]
trait PullSubscriptionBackend: Send + Sync {
    async fn ensure_topic(&self) -> Result<()>;
    async fn ensure_subscription(&self) -> Result<()>;
    async fn receive(
        &self,
        handler: WorkerEventHandler,
        event_type: String,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PullSubscriptionSpec {
    topic_name: &'static str,
    subscription_name: &'static str,
    event_type: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedPullSubscriptionSpec {
    topic_name: String,
    subscription_name: String,
    event_type: String,
}

// 2. Push Webhook Handler for Cloud Run
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GcpPushMessage {
    data: String,
    message_id: String,
}

#[derive(Deserialize)]
struct GcpPushPayload {
    message: GcpPushMessage,
    subscription: String,
}

fn worker_port<F>(mut get: F) -> String
where
    F: FnMut(&str) -> Option<String>,
{
    get("PORT").unwrap_or_else(|| "8081".to_string())
}

fn pull_mode_enabled<F>(mut get: F) -> bool
where
    F: FnMut(&str) -> Option<String>,
{
    get("PULL_MODE").is_some()
}

fn pubsub_project_id<F>(mut get: F) -> String
where
    F: FnMut(&str) -> Option<String>,
{
    get("GCP_PROJECT_ID").unwrap_or_else(|| "talon-local".to_string())
}

fn fully_qualified_topic(project_id: &str, topic_name: &str) -> String {
    fully_qualified_topic_name(project_id, topic_name)
}

fn fully_qualified_subscription(project_id: &str, subscription_name: &str) -> String {
    fully_qualified_subscription_name(project_id, subscription_name)
}

fn worker_bind_addr(port: &str) -> String {
    format!("0.0.0.0:{}", port)
}

fn pull_subscription_specs() -> [PullSubscriptionSpec; 3] {
    [
        PullSubscriptionSpec {
            topic_name: topics::SESSION_DISPATCH_TOPIC,
            subscription_name: "talon-session-dispatch-sub",
            event_type: "session_dispatch",
        },
        PullSubscriptionSpec {
            topic_name: topics::RESOURCE_LIFECYCLE_TOPIC,
            subscription_name: "talon-resource-lifecycle-sub",
            event_type: "resource_lifecycle",
        },
        PullSubscriptionSpec {
            topic_name: topics::SESSION_CONTROL_TOPIC,
            subscription_name: "talon-session-control-sub",
            event_type: "session_control",
        },
    ]
}

fn resolved_pull_subscription_specs(project_id: &str) -> Vec<ResolvedPullSubscriptionSpec> {
    pull_subscription_specs()
        .into_iter()
        .map(|spec| ResolvedPullSubscriptionSpec {
            topic_name: fully_qualified_topic(project_id, spec.topic_name),
            subscription_name: fully_qualified_subscription(project_id, spec.subscription_name),
            event_type: spec.event_type.to_string(),
        })
        .collect()
}

fn build_worker_handler(
    cp: Arc<ControlPlane>,
    config: Arc<Config>,
    scheduler_authenticator: Arc<SchedulerRequestAuthenticator>,
) -> WorkerEventHandler {
    WorkerEventHandler {
        cp,
        config,
        mcp_registry: Arc::new(talon::worker::mcp_registry::McpRegistry::new()),
        scheduler_authenticator,
        session_cancellations: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
    }
}

fn worker_router(handler: WorkerEventHandler) -> Router {
    Router::new()
        .route("/pubsub/push", post(push_webhook))
        .route("/schedules/fire", post(schedule_fire))
        .nest(
            "/mcp/talon-ops",
            talon::worker::talon_ops::talon_ops_router(handler.clone()),
        )
        .with_state(handler)
}

struct GcpPullSubscriptionBackend {
    client: google_cloud_pubsub::client::Client,
    topic_name: String,
    subscription_name: String,
}

impl GcpPullSubscriptionBackend {
    async fn new(project_id: String, topic_name: String, subscription_name: String) -> Result<Self> {
        use google_cloud_pubsub::client::{Client, ClientConfig};

        let mut pubsub_config = ClientConfig::default().with_auth().await?;
        pubsub_config.project_id = Some(project_id);
        let client = Client::new(pubsub_config).await?;
        Ok(Self {
            client,
            topic_name,
            subscription_name,
        })
    }
}

#[async_trait::async_trait]
impl PullSubscriptionBackend for GcpPullSubscriptionBackend {
    async fn ensure_topic(&self) -> Result<()> {
        let mut topic = self.client.topic(&self.topic_name);
        if !topic.exists(None).await? {
            if let Err(err) = topic.create(None, None).await {
                if !topic.exists(None).await? {
                    return Err(err.into());
                }
            }
        }
        Ok(())
    }

    async fn ensure_subscription(&self) -> Result<()> {
        use google_cloud_pubsub::subscription::SubscriptionConfig;

        let mut subscription = self.client.subscription(&self.subscription_name);
        if !subscription.exists(None).await? {
            let sub_config = SubscriptionConfig {
                ack_deadline_seconds: 300,
                ..Default::default()
            };
            if let Err(err) = subscription
                .create(&self.topic_name, sub_config, None)
                .await
            {
                if !subscription.exists(None).await? {
                    return Err(err.into());
                }
            }
        }
        Ok(())
    }

    async fn receive(
        &self,
        handler: WorkerEventHandler,
        event_type: String,
        cancellation_token: CancellationToken,
    ) -> Result<()> {
        let mut subscription = self.client.subscription(&self.subscription_name);
        let receive_loop_cancellation = cancellation_token.clone();
        subscription
            .receive(
                move |message, receive_cancellation_token| {
                    let h = handler.clone();
                    let event_type = event_type.clone();
                    let cancellation_token = receive_loop_cancellation.clone();
                    async move {
                        tokio::select! {
                            _ = cancellation_token.cancelled() => {}
                            _ = receive_cancellation_token.cancelled() => {}
                            result = h.dispatch(Some(&event_type), &message.message.data) => {
                                if let Err(e) = result {
                                    tracing::error!(event_type = %event_type, error = %e, "Pull dispatch failed");
                                    let _ = message.nack().await;
                                } else {
                                    let _ = message.ack().await;
                                }
                            }
                        }
                    }
                },
                cancellation_token.clone(),
                None,
            )
            .await?;
        Ok(())
    }
}

async fn run_pull_subscription_with_backend(
    backend: &dyn PullSubscriptionBackend,
    pull_handler: WorkerEventHandler,
    spec: ResolvedPullSubscriptionSpec,
    cancellation_token: CancellationToken,
) -> Result<()> {
    let fq_topic = spec.topic_name.clone();
    let fq_subscription = spec.subscription_name.clone();
    let event_type = spec.event_type.clone();

    backend.ensure_topic().await.map_err(|err| {
        anyhow::anyhow!(
            "Failed to create or inspect PubSub topic for worker subscription {}: {}",
            fq_topic,
            err
        )
    })?;
    backend.ensure_subscription().await.map_err(|err| {
        anyhow::anyhow!(
            "Failed to create or inspect PubSub subscription for worker {}: {}",
            fq_subscription,
            err
        )
    })?;
    backend
        .receive(pull_handler, event_type, cancellation_token)
        .await
        .map_err(|err| {
            anyhow::anyhow!(
                "PubSub receive loop exited with error for {}: {}",
                fq_subscription,
                err
            )
        })?;
    Ok(())
}

async fn run_pull_subscription_loop<F, Fut>(
    mut build_backend: F,
    pull_handler: WorkerEventHandler,
    spec: ResolvedPullSubscriptionSpec,
    shutdown_token: CancellationToken,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Box<dyn PullSubscriptionBackend>>>,
{
    let fq_topic = spec.topic_name.clone();
    let fq_subscription = spec.subscription_name.clone();
    let mut attempts = 0u32;

    loop {
        let backend = loop {
            match build_backend().await {
                Ok(backend) => {
                    attempts = 0;
                    break backend;
                }
                Err(e) => {
                    attempts += 1;
                    let backoff_secs = 2u64.saturating_pow(attempts.min(4));
                    tracing::error!(
                        topic = %fq_topic,
                        subscription = %fq_subscription,
                        attempt = attempts,
                        error = ?e,
                        "Failed to initialize PubSub client for worker subscription"
                    );
                    tokio::select! {
                        _ = shutdown_token.cancelled() => return,
                        _ = tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)) => {}
                    }
                }
            }
        };

        match run_pull_subscription_with_backend(
            backend.as_ref(),
            pull_handler.clone(),
            spec.clone(),
            shutdown_token.child_token(),
        )
        .await
        {
            Ok(()) => {
                tracing::warn!(
                    topic = %fq_topic,
                    subscription = %fq_subscription,
                    "PubSub receive loop exited normally"
                );
                return;
            }
            Err(e) => {
                attempts += 1;
                let backoff_secs = 2u64.saturating_pow(attempts.min(4));
                tracing::error!(
                    topic = %fq_topic,
                    subscription = %fq_subscription,
                    attempt = attempts,
                    error = ?e,
                    "PubSub receive loop exited with error"
                );
                tokio::select! {
                    _ = shutdown_token.cancelled() => return,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)) => {}
                }
            }
        }
    }
}

fn spawn_pull_subscription_task(
    pull_handler: WorkerEventHandler,
    project_id: String,
    spec: ResolvedPullSubscriptionSpec,
    shutdown_token: CancellationToken,
) {
    let ResolvedPullSubscriptionSpec {
        topic_name,
        subscription_name,
        event_type,
    } = spec;
    tokio::spawn(async move {
        let spec = ResolvedPullSubscriptionSpec {
            topic_name: topic_name.clone(),
            subscription_name: subscription_name.clone(),
            event_type,
        };
        run_pull_subscription_loop(
            || {
                let project_id = project_id.clone();
                let topic_name = topic_name.clone();
                let subscription_name = subscription_name.clone();
                async move {
                    Ok::<Box<dyn PullSubscriptionBackend>, anyhow::Error>(Box::new(
                        GcpPullSubscriptionBackend::new(project_id, topic_name, subscription_name)
                            .await?,
                    ))
                }
            },
            pull_handler,
            spec,
            shutdown_token,
        )
        .await;
    });
}

fn maybe_spawn_pull_subscriptions<F>(
    handler: WorkerEventHandler,
    pull_mode: bool,
    project_id: String,
    shutdown_token: CancellationToken,
    mut spawn: F,
) where
    F: FnMut(WorkerEventHandler, String, ResolvedPullSubscriptionSpec, CancellationToken),
{
    if !pull_mode {
        return;
    }

    tracing::info!("Starting in PULL mode (background thread)...");
    for spec in resolved_pull_subscription_specs(&project_id) {
        spawn(
            handler.clone(),
            project_id.clone(),
            spec,
            shutdown_token.child_token(),
        );
    }
}

async fn serve_worker_http(
    handler: WorkerEventHandler,
    port: String,
    shutdown_token: CancellationToken,
) -> Result<()> {
    let app = worker_router(handler);
    tracing::info!(
        port = %port,
        "Worker listening for Push events / Health checks on 0.0.0.0:{}",
        port
    );
    let listener = tokio::net::TcpListener::bind(worker_bind_addr(&port)).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_token.cancelled_owned())
        .await?;
    Ok(())
}

async fn run_worker_with<FGet, FSpawn, FServe, Fut, FShutdown>(
    cp: Arc<ControlPlane>,
    config: Arc<Config>,
    scheduler_authenticator: Arc<SchedulerRequestAuthenticator>,
    env_get: FGet,
    spawn: FSpawn,
    serve: FServe,
    shutdown: FShutdown,
) -> Result<()>
where
    FGet: Fn(&str) -> Option<String>,
    FSpawn: Fn(WorkerEventHandler, bool, String, CancellationToken),
    FServe: Fn(WorkerEventHandler, String, CancellationToken) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
    FShutdown: std::future::Future,
{
    let handler = build_worker_handler(cp, config, scheduler_authenticator);
    let pull_mode = pull_mode_enabled(&env_get);
    let project_id = pubsub_project_id(&env_get);
    let shutdown_token = CancellationToken::new();
    spawn(
        handler.clone(),
        pull_mode,
        project_id,
        shutdown_token.child_token(),
    );
    tokio::pin!(shutdown);
    let serve_future = serve(
        handler,
        worker_port(env_get),
        shutdown_token.child_token(),
    );
    tokio::pin!(serve_future);
    tokio::select! {
        res = &mut serve_future => return res,
        _ = &mut shutdown => {
            tracing::info!("Shutting down worker...");
            shutdown_token.cancel();
        }
    }
    serve_future.await
}

async fn run_worker_main_with<FLoad, FBuildCp, FBuildCpFuture, FBuildAuth, FBuildAuthFuture, FGet, FSpawn, FServe, Fut, FShutdown>(
    load_config: FLoad,
    build_cp: FBuildCp,
    build_auth: FBuildAuth,
    env_get: FGet,
    spawn: FSpawn,
    serve: FServe,
    shutdown: FShutdown,
) -> Result<()>
where
    FLoad: FnOnce() -> Result<Arc<Config>>,
    FBuildCp: FnOnce(&Arc<Config>) -> FBuildCpFuture,
    FBuildCpFuture: std::future::Future<Output = Result<Arc<ControlPlane>>>,
    FBuildAuth: FnOnce(&Arc<Config>) -> FBuildAuthFuture,
    FBuildAuthFuture: std::future::Future<Output = Result<Arc<SchedulerRequestAuthenticator>>>,
    FGet: Fn(&str) -> Option<String>,
    FSpawn: Fn(WorkerEventHandler, bool, String, CancellationToken),
    FServe: Fn(WorkerEventHandler, String, CancellationToken) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
    FShutdown: std::future::Future,
{
    let config = load_config()?;
    let cp = build_cp(&config).await?;
    let scheduler_authenticator = build_auth(&config).await?;
    run_worker_with(
        cp,
        config,
        scheduler_authenticator,
        env_get,
        spawn,
        serve,
        shutdown,
    )
    .await
}

async fn push_webhook(
    State(handler): State<WorkerEventHandler>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    println!(
        "Received Webhook Payload: {}",
        serde_json::to_string_pretty(&payload).unwrap()
    );

    if let Ok(parsed) = serde_json::from_value::<GcpPushPayload>(payload) {
        use base64::{engine::general_purpose, Engine as _};
        if let Ok(raw_bytes) = general_purpose::STANDARD.decode(&parsed.message.data) {
            let event_type = WorkerEventHandler::event_type_for_subscription(&parsed.subscription);
            match handler.dispatch(event_type, &raw_bytes).await {
                Ok(_) => axum::http::StatusCode::OK,
                Err(e) => {
                    tracing::error!(
                        message_id = %parsed.message.message_id,
                        error = %e,
                        "Failed to handle push event"
                    );
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR
                }
            }
        } else {
            axum::http::StatusCode::BAD_REQUEST
        }
    } else {
        tracing::warn!("Could not decode payload as GcpPushPayload");
        axum::http::StatusCode::UNPROCESSABLE_ENTITY
    }
}

async fn schedule_fire(
    State(handler): State<WorkerEventHandler>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(err) = handler.scheduler_authenticator.authorize(&headers).await {
        tracing::warn!(error = %err, "Rejected scheduler wakeup request");
        return axum::http::StatusCode::UNAUTHORIZED;
    }

    let payload = match serde_json::from_slice::<talon::scheduling::ScheduleWakeupPayload>(&body) {
        Ok(payload) => payload,
        Err(err) => {
            tracing::warn!(error = %err, "Invalid schedule wakeup payload");
            return axum::http::StatusCode::BAD_REQUEST;
        }
    };

    match handler.handle_schedule_wakeup(payload).await {
        Ok(_) => axum::http::StatusCode::OK,
        Err(err) => {
            tracing::error!(error = %err, "Failed to process schedule wakeup");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    talon::security::install_jwt_crypto_provider();
    tracing_subscriber::fmt::init();
    tracing::info!("Starting Talon Worker Engine...");
    tracing::info!("Connecting to control plane services...");
    run_worker_main_with(
        || Ok(Arc::new(Config::load_default()?)),
        |config| {
            let config = Arc::clone(config);
            async move { Ok(Arc::new(build_control_plane(&config).await?)) }
        },
        |config| {
            let config = Arc::clone(config);
            async move { Ok(Arc::new(SchedulerRequestAuthenticator::from_config(&config).await?)) }
        },
        |name| std::env::var(name).ok(),
        |pull_handler, pull_mode, project_id, shutdown_token| {
            maybe_spawn_pull_subscriptions(
                pull_handler,
                pull_mode,
                project_id,
                shutdown_token,
                spawn_pull_subscription_task,
            );
        },
        |handler, port, shutdown| async move { serve_worker_http(handler, port, shutdown).await },
        signal::ctrl_c(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{
        build_worker_handler, fully_qualified_subscription, fully_qualified_topic,
        maybe_spawn_pull_subscriptions, pull_mode_enabled, pull_subscription_specs,
        pubsub_project_id, push_webhook, resolved_pull_subscription_specs, run_worker_main_with,
        run_pull_subscription_loop, run_pull_subscription_with_backend, run_worker_with, PullSubscriptionBackend,
        ResolvedPullSubscriptionSpec,
        schedule_fire, serve_worker_http, worker_bind_addr, worker_port, worker_router,
    };
    use anyhow::Result;
    use axum::body::Bytes;
    use axum::extract::State;
    use axum::http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode};
    use axum::response::IntoResponse;
    use axum::Json;
    use base64::{engine::general_purpose, Engine as _};
    use futures::StreamExt;
    use prost::Message;
    use serde_json::json;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Arc;
    use talon::config::Config;
    use talon::control::{
        events::LifecycleEvent,
        keys,
        scheduler::NoopSchedulerBackend,
        ControlPlane, KeyValueStore, MessagePublisher, ProtoKeyValueStoreExt,
    };
    use talon::gateway::rpc::models;
    use talon::worker::{
        mcp_registry::McpRegistry, scheduler_auth::SchedulerRequestAuthenticator, WorkerEventHandler,
    };
    use tokio::sync::Mutex;
    use tokio_util::sync::CancellationToken;
    use tower::ServiceExt;

    #[derive(Default)]
    struct MockKvStore {
        data: Mutex<HashMap<(String, String), Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, ns: &str, k: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self
                .data
                .lock()
                .await
                .get(&(ns.to_string(), k.to_string()))
                .cloned())
        }

        async fn set(&self, ns: &str, k: &str, v: &[u8]) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .insert((ns.to_string(), k.to_string()), v.to_vec());
            Ok(())
        }

        async fn compare_and_swap(
            &self,
            ns: &str,
            k: &str,
            expected: Option<&[u8]>,
            value: &[u8],
        ) -> anyhow::Result<bool> {
            let mut data = self.data.lock().await;
            let key = (ns.to_string(), k.to_string());
            let current = data.get(&key).cloned();
            let matches = match (current.as_deref(), expected) {
                (None, None) => true,
                (Some(current), Some(expected)) => current == expected,
                _ => false,
            };
            if matches {
                data.insert(key, value.to_vec());
            }
            Ok(matches)
        }

        async fn delete(&self, ns: &str, k: &str) -> anyhow::Result<()> {
            self.data
                .lock()
                .await
                .remove(&(ns.to_string(), k.to_string()));
            Ok(())
        }

        async fn list_keys(&self, ns: &str, p: &str) -> anyhow::Result<Vec<String>> {
            let mut keys = self
                .data
                .lock()
                .await
                .keys()
                .filter_map(|(stored_ns, key)| {
                    (stored_ns == ns && key.starts_with(p)).then(|| key.clone())
                })
                .collect::<Vec<_>>();
            keys.sort();
            Ok(keys)
        }
    }

    #[derive(Default)]
    struct MockPubSub;

    #[async_trait::async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, _topic: &str, _message: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>> {
            Ok(Box::pin(futures::stream::empty()))
        }
    }

    fn handler_with_auth(authenticator: SchedulerRequestAuthenticator) -> WorkerEventHandler {
        WorkerEventHandler {
            cp: Arc::new(ControlPlane {
                kv: Arc::new(MockKvStore::default()),
                pubsub: Arc::new(MockPubSub),
                scheduler: Arc::new(NoopSchedulerBackend),
            }),
            config: Arc::new(Config::default()),
            mcp_registry: Arc::new(McpRegistry::new()),
            scheduler_authenticator: Arc::new(authenticator),
            session_cancellations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[derive(Default)]
    struct FakePullBackend {
        calls: std::sync::Mutex<Vec<&'static str>>,
        fail_topic: bool,
        fail_subscription: bool,
        fail_receive: bool,
        cancel_on_receive: bool,
    }

    #[async_trait::async_trait]
    impl PullSubscriptionBackend for FakePullBackend {
        async fn ensure_topic(&self) -> Result<()> {
            self.calls
                .lock()
                .expect("calls lock poisoned")
                .push("topic");
            if self.fail_topic {
                anyhow::bail!("topic failed");
            }
            Ok(())
        }

        async fn ensure_subscription(&self) -> Result<()> {
            self.calls
                .lock()
                .expect("calls lock poisoned")
                .push("subscription");
            if self.fail_subscription {
                anyhow::bail!("subscription failed");
            }
            Ok(())
        }

        async fn receive(
            &self,
            _handler: WorkerEventHandler,
            _event_type: String,
            _cancellation_token: tokio_util::sync::CancellationToken,
        ) -> Result<()> {
            self.calls
                .lock()
                .expect("calls lock poisoned")
                .push("receive");
            if self.cancel_on_receive {
                _cancellation_token.cancel();
            }
            if self.fail_receive {
                anyhow::bail!("receive failed");
            }
            Ok(())
        }
    }

    fn schedule_with_next_run(revision: u64, next_run_at: i64) -> models::Schedule {
        models::Schedule {
            name: "nightly".to_string(),
            ns: "default".to_string(),
            labels: HashMap::new(),
            spec: Some(models::ScheduleSpec {
                kind: "every".to_string(),
                cron: String::new(),
                interval_seconds: 600,
                run_at: String::new(),
                timezone: String::new(),
                target: Some(models::ScheduleTarget {
                    agent: "assistant".to_string(),
                    session_mode: "reuse".to_string(),
                    session_id: "session-1".to_string(),
                }),
                input_message: "Run".to_string(),
                enabled: true,
            }),
            status: Some(models::ScheduleStatus {
                revision,
                next_run_at: Some(next_run_at),
                backend_handle: None,
                backend_armed: false,
                last_run_at: None,
                last_session_id: None,
                last_error: None,
                claimed_run_at: None,
                claim_expires_at: None,
                recent_events: Vec::new(),
            }),
        }
    }

    #[test]
    fn worker_helpers_use_defaults_and_presence_checks() {
        assert_eq!(worker_port(|_| None), "8081");
        assert_eq!(worker_port(|_| Some("9090".to_string())), "9090");
        assert!(!pull_mode_enabled(|_| None));
        assert!(pull_mode_enabled(|_| Some(String::new())));
        assert_eq!(pubsub_project_id(|_| None), "talon-local");
        assert_eq!(
            pubsub_project_id(|_| Some("project-123".to_string())),
            "project-123"
        );
    }

    #[test]
    fn pull_mode_helpers_cover_specs_and_qualified_names() {
        let specs = pull_subscription_specs();
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].topic_name, talon::control::topics::SESSION_DISPATCH_TOPIC);
        assert_eq!(specs[1].event_type, "resource_lifecycle");
        assert_eq!(specs[2].subscription_name, "talon-session-control-sub");

        assert_eq!(
            fully_qualified_topic("demo", "events"),
            "projects/demo/topics/events"
        );
        assert_eq!(
            fully_qualified_topic("demo", "projects/other/topics/events"),
            "projects/other/topics/events"
        );
        assert_eq!(
            fully_qualified_subscription("demo", "events-sub"),
            "projects/demo/subscriptions/events-sub"
        );
        assert_eq!(
            fully_qualified_subscription(
                "demo",
                "projects/other/subscriptions/events-sub"
            ),
            "projects/other/subscriptions/events-sub"
        );
        assert_eq!(worker_bind_addr("8081"), "0.0.0.0:8081");
    }

    #[test]
    fn resolved_pull_specs_and_handler_builder_cover_startup_wiring() {
        let specs = resolved_pull_subscription_specs("demo");
        assert_eq!(specs.len(), 3);
        assert_eq!(
            specs[0].topic_name,
            "projects/demo/topics/talon.session.dispatch"
        );
        assert_eq!(
            specs[1].subscription_name,
            "projects/demo/subscriptions/talon-resource-lifecycle-sub"
        );
        assert_eq!(specs[2].event_type, "session_control");

        let cp = Arc::new(ControlPlane {
            kv: Arc::new(MockKvStore::default()),
            pubsub: Arc::new(MockPubSub),
            scheduler: Arc::new(NoopSchedulerBackend),
        });
        let config = Arc::new(Config::default());
        let auth = Arc::new(SchedulerRequestAuthenticator::deny_all());
        let handler = build_worker_handler(cp.clone(), config.clone(), auth.clone());
        assert!(Arc::ptr_eq(&handler.cp, &cp));
        assert!(Arc::ptr_eq(&handler.config, &config));
        assert!(Arc::ptr_eq(&handler.scheduler_authenticator, &auth));
        assert!(handler.session_cancellations.blocking_lock().is_empty());
    }

    #[tokio::test]
    async fn mock_control_plane_helpers_cover_storage_and_pubsub_branches() {
        let kv = MockKvStore::default();
        assert_eq!(kv.get("root", "missing").await.unwrap(), None);

        kv.set("root", "agents/a", b"one").await.unwrap();
        kv.set("root", "agents/b", b"two").await.unwrap();
        kv.set("other", "agents/c", b"three").await.unwrap();
        assert_eq!(kv.get("root", "agents/a").await.unwrap(), Some(b"one".to_vec()));

        assert!(kv
            .compare_and_swap("root", "agents/new", None, b"created")
            .await
            .unwrap());
        assert!(kv
            .compare_and_swap("root", "agents/a", Some(b"one"), b"updated")
            .await
            .unwrap());
        assert!(!kv
            .compare_and_swap("root", "agents/a", Some(b"wrong"), b"nope")
            .await
            .unwrap());

        let keys = kv.list_keys("root", "agents/").await.unwrap();
        assert_eq!(
            keys,
            vec![
                "agents/a".to_string(),
                "agents/b".to_string(),
                "agents/new".to_string(),
            ]
        );

        kv.delete("root", "agents/b").await.unwrap();
        assert_eq!(kv.get("root", "agents/b").await.unwrap(), None);

        let pubsub = MockPubSub;
        pubsub.publish("topic", b"payload").await.unwrap();
        let items = pubsub
            .subscribe("topic")
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;
        assert!(items.is_empty());
    }

    #[test]
    fn maybe_spawn_pull_subscriptions_respects_pull_mode_and_emits_specs() {
        let handler = handler_with_auth(SchedulerRequestAuthenticator::deny_all());
        let spawned = std::sync::Mutex::new(Vec::<(String, String, String)>::new());

        maybe_spawn_pull_subscriptions(
            handler.clone(),
            false,
            "demo".to_string(),
            CancellationToken::new(),
            |_h, project_id, spec, _shutdown_token| {
            spawned
                .lock()
                .expect("spawned lock poisoned")
                .push((project_id, spec.topic_name, spec.subscription_name));
            },
        );
        assert!(spawned.lock().expect("spawned lock poisoned").is_empty());

        maybe_spawn_pull_subscriptions(
            handler,
            true,
            "demo".to_string(),
            CancellationToken::new(),
            |_h, project_id, spec, _shutdown_token| {
            spawned
                .lock()
                .expect("spawned lock poisoned")
                .push((project_id, spec.topic_name, spec.subscription_name));
            },
        );
        let spawned = spawned.lock().expect("spawned lock poisoned");
        assert_eq!(spawned.len(), 3);
        assert!(spawned.iter().all(|(project_id, _, _)| project_id == "demo"));
        assert!(spawned[0].1.starts_with("projects/demo/topics/"));
        assert!(spawned[0].2.starts_with("projects/demo/subscriptions/"));
    }

    #[tokio::test]
    async fn worker_router_mounts_expected_routes() {
        let app = worker_router(handler_with_auth(SchedulerRequestAuthenticator::deny_all()));

        let get_push = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/pubsub/push")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should succeed");
        assert_eq!(get_push.status(), StatusCode::METHOD_NOT_ALLOWED);

        let post_push = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/pubsub/push")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(r#"{"unexpected":true}"#))
                    .unwrap(),
            )
            .await
            .expect("request should succeed");
        assert_eq!(post_push.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let schedule_fire = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/schedules/fire")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .expect("request should succeed");
        assert_eq!(schedule_fire.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn push_webhook_rejects_invalid_payload_shapes_and_base64() {
        let handler = handler_with_auth(SchedulerRequestAuthenticator::deny_all());

        let invalid_shape = push_webhook(
            State(handler.clone()),
            Json(json!({"unexpected": true})),
        )
        .await
        .into_response();
        assert_eq!(invalid_shape.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let invalid_base64 = push_webhook(
            State(handler),
            Json(json!({
                "message": {"data": "!!!", "messageId": "m1"},
                "subscription": "projects/test/subscriptions/unknown"
            })),
        )
        .await
        .into_response();
        assert_eq!(invalid_base64.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn push_webhook_dispatches_known_lifecycle_events() {
        let handler = handler_with_auth(SchedulerRequestAuthenticator::deny_all());
        let event = LifecycleEvent {
            resource_type: "McpServerBinding".to_string(),
            name: "binding-1".to_string(),
            ns: "default".to_string(),
            action: 1,
            timestamp: 123,
        };
        let payload = json!({
            "message": {
                "data": general_purpose::STANDARD.encode(event.encode_to_vec()),
                "messageId": "m1"
            },
            "subscription": format!("projects/test/subscriptions/{}", talon::control::topics::RESOURCE_LIFECYCLE_TOPIC)
        });

        let response = push_webhook(State(handler), Json(payload))
            .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn push_webhook_surfaces_dispatch_failures_as_internal_errors() {
        let handler = handler_with_auth(SchedulerRequestAuthenticator::deny_all());
        let payload = json!({
            "message": {
                "data": general_purpose::STANDARD.encode(b"not-a-protobuf"),
                "messageId": "m1"
            },
            "subscription": format!(
                "projects/test/subscriptions/{}",
                talon::control::topics::SESSION_DISPATCH_TOPIC
            )
        });

        let response = push_webhook(State(handler), Json(payload))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn schedule_fire_rejects_unauthorized_and_invalid_payloads() {
        let unauthorized = schedule_fire(
            State(handler_with_auth(SchedulerRequestAuthenticator::deny_all())),
            HeaderMap::new(),
            Bytes::from_static(br#"{"scheduleId":"sched-1"}"#),
        )
        .await
        .into_response();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        let invalid_payload = schedule_fire(
            State(handler_with_auth(SchedulerRequestAuthenticator::shared_secret(
                "secret".to_string(),
            ))),
            headers,
            Bytes::from_static(br#"not-json"#),
        )
        .await
        .into_response();
        assert_eq!(invalid_payload.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn schedule_fire_accepts_valid_payload_when_no_matching_schedule_exists() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        let payload = json!({
            "namespace": "default",
            "schedule_id": "missing-schedule",
            "revision": 1,
            "intended_run_at": chrono::Utc::now().timestamp_micros()
        });

        let response = schedule_fire(
            State(handler_with_auth(SchedulerRequestAuthenticator::shared_secret(
                "secret".to_string(),
            ))),
            headers,
            Bytes::from(serde_json::to_vec(&payload).unwrap()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn schedule_fire_surfaces_wakeup_processing_failures() {
        let kv = Arc::new(MockKvStore::default());
        kv.set_msg(
            "default",
            &keys::schedule("nightly"),
            &schedule_with_next_run(4, i64::MAX),
        )
        .await
        .unwrap();

        let handler = WorkerEventHandler {
            cp: Arc::new(ControlPlane {
                kv,
                pubsub: Arc::new(MockPubSub),
                scheduler: Arc::new(NoopSchedulerBackend),
            }),
            config: Arc::new(Config::default()),
            mcp_registry: Arc::new(McpRegistry::new()),
            scheduler_authenticator: Arc::new(SchedulerRequestAuthenticator::shared_secret(
                "secret".to_string(),
            )),
            session_cancellations: Arc::new(Mutex::new(HashMap::new())),
        };

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        let payload = json!({
            "namespace": "default",
            "schedule_id": "nightly",
            "revision": 4,
            "intended_run_at": i64::MAX
        });

        let response = schedule_fire(
            State(handler),
            headers,
            Bytes::from(serde_json::to_vec(&payload).unwrap()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn serve_worker_http_binds_listener_and_surfaces_address_errors() {
        let handler = handler_with_auth(SchedulerRequestAuthenticator::deny_all());
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("probe should bind");
        let port = probe.local_addr().expect("probe addr").port();
        drop(probe);

        let task = tokio::spawn(serve_worker_http(
            handler.clone(),
            port.to_string(),
            CancellationToken::new(),
        ));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        task.abort();
        let err = task.await.expect_err("task should abort");
        assert!(err.is_cancelled());

        let err = serve_worker_http(handler, "not-a-port".to_string(), CancellationToken::new())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid port value"));
    }

    #[tokio::test]
    async fn run_worker_with_routes_pull_mode_and_http_startup() {
        let cp = Arc::new(ControlPlane {
            kv: Arc::new(MockKvStore::default()),
            pubsub: Arc::new(MockPubSub),
            scheduler: Arc::new(NoopSchedulerBackend),
        });
        let config = Arc::new(Config::default());
        let auth = Arc::new(SchedulerRequestAuthenticator::deny_all());
        let spawned = Arc::new(std::sync::Mutex::new(Vec::<(bool, String)>::new()));

        let result = run_worker_with(
            cp,
            config,
            auth,
            |name| match name {
                "PULL_MODE" => Some("1".to_string()),
                "GCP_PROJECT_ID" => Some("project-123".to_string()),
                "PORT" => Some("9099".to_string()),
                _ => None,
            },
            {
                let spawned = spawned.clone();
                move |_handler, pull_mode, project_id, _shutdown_token| {
                    spawned
                        .lock()
                        .expect("spawned lock poisoned")
                        .push((pull_mode, project_id));
                }
            },
            |_handler, port, _shutdown_token| async move {
                assert_eq!(port, "9099");
                Ok(())
            },
            futures::future::pending::<()>(),
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(
            *spawned.lock().expect("spawned lock poisoned"),
            vec![(true, "project-123".to_string())]
        );
    }

    #[tokio::test]
    async fn run_worker_with_surfaces_http_errors_without_pull_mode() {
        let cp = Arc::new(ControlPlane {
            kv: Arc::new(MockKvStore::default()),
            pubsub: Arc::new(MockPubSub),
            scheduler: Arc::new(NoopSchedulerBackend),
        });
        let config = Arc::new(Config::default());
        let auth = Arc::new(SchedulerRequestAuthenticator::deny_all());
        let spawned = Arc::new(std::sync::Mutex::new(Vec::<(bool, String)>::new()));

        let err = run_worker_with(
            cp,
            config,
            auth,
            |_| None,
            {
                let spawned = spawned.clone();
                move |_handler, pull_mode, project_id, _shutdown_token| {
                    spawned
                        .lock()
                        .expect("spawned lock poisoned")
                        .push((pull_mode, project_id));
                }
            },
            |_handler, _port, _shutdown_token| async { anyhow::bail!("serve failed") },
            futures::future::pending::<()>(),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("serve failed"));
        assert_eq!(
            *spawned.lock().expect("spawned lock poisoned"),
            vec![(false, "talon-local".to_string())]
        );
    }

    #[tokio::test]
    async fn run_worker_with_awaits_http_shutdown_after_signal() {
        let cp = Arc::new(ControlPlane {
            kv: Arc::new(MockKvStore::default()),
            pubsub: Arc::new(MockPubSub),
            scheduler: Arc::new(NoopSchedulerBackend),
        });
        let config = Arc::new(Config::default());
        let auth = Arc::new(SchedulerRequestAuthenticator::deny_all());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let result = run_worker_with(
            cp,
            config,
            auth,
            |_| None,
            |_handler, _pull_mode, _project_id, _shutdown_token| {},
            {
                let cancelled = cancelled.clone();
                move |_handler, _port, shutdown_token| {
                    let cancelled = cancelled.clone();
                    async move {
                        shutdown_token.cancelled().await;
                        cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
                        Ok(())
                    }
                }
            },
            async move {
                let _ = shutdown_rx.await;
            },
        );

        let signal_task = tokio::spawn(async move {
            let _ = shutdown_tx.send(());
        });

        result.await.unwrap();
        assert!(
            cancelled.load(std::sync::atomic::Ordering::SeqCst),
            "serve future should observe cancellation"
        );
        signal_task.await.unwrap();
    }

    #[tokio::test]
    async fn run_worker_main_with_surfaces_bootstrap_failures() {
        let config_err = run_worker_main_with(
            || anyhow::bail!("config failed"),
            |_| async { Ok(Arc::new(ControlPlane {
                kv: Arc::new(MockKvStore::default()),
                pubsub: Arc::new(MockPubSub),
                scheduler: Arc::new(NoopSchedulerBackend),
            })) },
            |_| async { Ok(Arc::new(SchedulerRequestAuthenticator::deny_all())) },
            |_| None,
            |_handler, _pull_mode, _project_id, _shutdown_token| {},
            |_handler, _port, _shutdown_token| async { Ok(()) },
            futures::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        assert!(config_err.to_string().contains("config failed"));

        let cp_err = run_worker_main_with(
            || Ok(Arc::new(Config::default())),
            |_| async { anyhow::bail!("control plane failed") },
            |_| async { Ok(Arc::new(SchedulerRequestAuthenticator::deny_all())) },
            |_| None,
            |_handler, _pull_mode, _project_id, _shutdown_token| {},
            |_handler, _port, _shutdown_token| async { Ok(()) },
            futures::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        assert!(cp_err.to_string().contains("control plane failed"));

        let auth_err = run_worker_main_with(
            || Ok(Arc::new(Config::default())),
            |_| async { Ok(Arc::new(ControlPlane {
                kv: Arc::new(MockKvStore::default()),
                pubsub: Arc::new(MockPubSub),
                scheduler: Arc::new(NoopSchedulerBackend),
            })) },
            |_| async { anyhow::bail!("scheduler auth failed") },
            |_| None,
            |_handler, _pull_mode, _project_id, _shutdown_token| {},
            |_handler, _port, _shutdown_token| async { Ok(()) },
            futures::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        assert!(auth_err.to_string().contains("scheduler auth failed"));
    }

    #[tokio::test]
    async fn run_worker_main_with_starts_and_routes_to_worker_runtime() {
        let spawned = Arc::new(std::sync::Mutex::new(Vec::<(bool, String)>::new()));
        let result = run_worker_main_with(
            || Ok(Arc::new(Config::default())),
            |_| async { Ok(Arc::new(ControlPlane {
                kv: Arc::new(MockKvStore::default()),
                pubsub: Arc::new(MockPubSub),
                scheduler: Arc::new(NoopSchedulerBackend),
            })) },
            |_| async { Ok(Arc::new(SchedulerRequestAuthenticator::deny_all())) },
            |name| match name {
                "PULL_MODE" => Some("1".to_string()),
                "GCP_PROJECT_ID" => Some("project-123".to_string()),
                "PORT" => Some("8181".to_string()),
                _ => None,
            },
            {
                let spawned = spawned.clone();
                move |_handler, pull_mode, project_id, _shutdown_token| {
                    spawned
                        .lock()
                        .expect("spawned lock poisoned")
                        .push((pull_mode, project_id));
                }
            },
            |_handler, port, _shutdown_token| async move {
                assert_eq!(port, "8181");
                Ok(())
            },
            futures::future::pending::<()>(),
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(
            *spawned.lock().expect("spawned lock poisoned"),
            vec![(true, "project-123".to_string())]
        );
    }

    #[tokio::test]
    async fn run_pull_subscription_with_backend_covers_setup_and_receive_failures() {
        let spec = ResolvedPullSubscriptionSpec {
            topic_name: "projects/demo/topics/events".to_string(),
            subscription_name: "projects/demo/subscriptions/events-sub".to_string(),
            event_type: "resource_lifecycle".to_string(),
        };
        let handler = handler_with_auth(SchedulerRequestAuthenticator::deny_all());

        let topic_fail = FakePullBackend {
            fail_topic: true,
            ..Default::default()
        };
        let err = run_pull_subscription_with_backend(
            &topic_fail,
            handler.clone(),
            spec.clone(),
            tokio_util::sync::CancellationToken::new(),
        )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Failed to create or inspect PubSub topic"));
        assert_eq!(
            topic_fail.calls.lock().expect("calls lock poisoned").as_slice(),
            &["topic"]
        );

        let subscription_fail = FakePullBackend {
            fail_subscription: true,
            ..Default::default()
        };
        let err = run_pull_subscription_with_backend(
            &subscription_fail,
            handler.clone(),
            spec.clone(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("Failed to create or inspect PubSub subscription"));
        assert_eq!(
            subscription_fail
                .calls
                .lock()
                .expect("calls lock poisoned")
                .as_slice(),
            &["topic", "subscription"]
        );

        let receive_fail = FakePullBackend {
            fail_receive: true,
            ..Default::default()
        };
        let err = run_pull_subscription_with_backend(
            &receive_fail,
            handler.clone(),
            spec.clone(),
            tokio_util::sync::CancellationToken::new(),
        )
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("PubSub receive loop exited with error"));
        assert_eq!(
            receive_fail
                .calls
                .lock()
                .expect("calls lock poisoned")
                .as_slice(),
            &["topic", "subscription", "receive"]
        );

        let ok = FakePullBackend::default();
        run_pull_subscription_with_backend(
            &ok,
            handler,
            spec,
            tokio_util::sync::CancellationToken::new(),
        )
            .await
            .expect("successful path should return ok");
        assert_eq!(
            ok.calls.lock().expect("calls lock poisoned").as_slice(),
            &["topic", "subscription", "receive"]
        );
    }

    #[tokio::test]
    async fn run_pull_subscription_loop_retries_after_receive_failure() {
        let handler = handler_with_auth(SchedulerRequestAuthenticator::deny_all());
        let spec = ResolvedPullSubscriptionSpec {
            topic_name: "projects/demo/topics/events".to_string(),
            subscription_name: "projects/demo/subscriptions/events-sub".to_string(),
            event_type: "resource_lifecycle".to_string(),
        };
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let shutdown = CancellationToken::new();

        run_pull_subscription_loop(
            {
                let attempts = attempts.clone();
                move || {
                    let attempts = attempts.clone();
                    async move {
                        let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let backend = if attempt == 0 {
                            FakePullBackend {
                                fail_receive: true,
                                ..Default::default()
                            }
                        } else {
                            FakePullBackend {
                                cancel_on_receive: true,
                                ..Default::default()
                            }
                        };
                        Ok::<Box<dyn PullSubscriptionBackend>, anyhow::Error>(Box::new(backend))
                    }
                }
            },
            handler,
            spec,
            shutdown,
        )
        .await;

        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
