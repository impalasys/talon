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
use talon::control::topics;
use talon::worker::{scheduler_auth::SchedulerRequestAuthenticator, WorkerEventHandler};

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
                    eprintln!(
                        "Failed to handle event {}: {}",
                        parsed.message.message_id, e
                    );
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR
                }
            }
        } else {
            axum::http::StatusCode::BAD_REQUEST
        }
    } else {
        println!("Could not decode payload as GcpPushPayload!");
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
    let config = Config::load_default()?;
    let config = Arc::new(config);

    // Initialize Control Plane
    println!("Connecting to control plane services...");
    let cp = Arc::new(build_control_plane(&config).await?);

    let scheduler_authenticator =
        Arc::new(SchedulerRequestAuthenticator::from_config(&config).await?);

    let handler = WorkerEventHandler {
        cp: Arc::clone(&cp),
        config: Arc::clone(&config),
        mcp_registry: Arc::new(talon::worker::mcp_registry::McpRegistry::new()),
        scheduler_authenticator,
        session_cancellations: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
    };

    let pull_mode = std::env::var("PULL_MODE").is_ok();

    // Support for Pull Mode (local / kubernetes)
    if pull_mode {
        println!("Starting in PULL mode (background thread)...");
        for (topic_name, subscription_name, event_type) in [
            (
                topics::SESSION_DISPATCH_TOPIC,
                "talon-session-dispatch-sub",
                "session_dispatch",
            ),
            (
                topics::RESOURCE_LIFECYCLE_TOPIC,
                "talon-resource-lifecycle-sub",
                "resource_lifecycle",
            ),
            (
                topics::SESSION_CONTROL_TOPIC,
                "talon-session-control-sub",
                "session_control",
            ),
        ] {
            let pull_handler = handler.clone();
            let topic_name = topic_name.to_string();
            let subscription_name = subscription_name.to_string();
            let event_type = event_type.to_string();
            tokio::spawn(async move {
                use google_cloud_pubsub::client::{Client, ClientConfig};
                use google_cloud_pubsub::subscription::SubscriptionConfig;
                use tokio_util::sync::CancellationToken;

                let mut pubsub_config = ClientConfig::default().with_auth().await.unwrap();
                let project_id =
                    std::env::var("GCP_PROJECT_ID").unwrap_or_else(|_| "talon-local".to_string());
                pubsub_config.project_id = Some(project_id.clone());
                let client = Client::new(pubsub_config).await.unwrap();
                let fq_topic = if topic_name.starts_with("projects/") {
                    topic_name.clone()
                } else {
                    format!("projects/{}/topics/{}", project_id, topic_name)
                };
                let fq_subscription = if subscription_name.starts_with("projects/") {
                    subscription_name.clone()
                } else {
                    format!(
                        "projects/{}/subscriptions/{}",
                        project_id, subscription_name
                    )
                };

                let mut topic = client.topic(&fq_topic);
                match topic.exists(None).await {
                    Ok(false) => {
                        if let Err(err) = topic.create(None, None).await {
                            tracing::error!(
                                topic = %fq_topic,
                                error = %err,
                                "Failed to create PubSub topic for worker subscription"
                            );
                            return;
                        }
                    }
                    Ok(true) => {}
                    Err(err) => {
                        tracing::error!(
                            topic = %fq_topic,
                            error = %err,
                            "Failed to inspect PubSub topic for worker subscription"
                        );
                        return;
                    }
                }

                let mut subscription = client.subscription(&fq_subscription);
                match subscription.exists(None).await {
                    Ok(false) => {
                        let sub_config = SubscriptionConfig {
                            ack_deadline_seconds: 300,
                            ..Default::default()
                        };
                        if let Err(err) =
                            subscription.create(&fq_topic, sub_config, None).await
                        {
                            tracing::error!(
                                subscription = %fq_subscription,
                                topic = %fq_topic,
                                error = %err,
                                "Failed to create PubSub subscription for worker"
                            );
                            return;
                        }
                    }
                    Ok(true) => {}
                    Err(err) => {
                        tracing::error!(
                            subscription = %fq_subscription,
                            error = %err,
                            "Failed to inspect PubSub subscription for worker"
                        );
                        return;
                    }
                }

                if let Err(e) = subscription
                    .receive(
                        move |mut message, _cancellation_token| {
                            let h = pull_handler.clone();
                            let event_type = event_type.clone();
                            async move {
                                if let Err(e) =
                                    h.dispatch(Some(&event_type), &message.message.data).await
                                {
                                    eprintln!("Pull dispatch failed: {}", e);
                                }
                                let _ = message.ack().await;
                            }
                        },
                        CancellationToken::new(),
                        None,
                    )
                    .await
                {
                    tracing::error!(
                        topic = %fq_topic,
                        subscription = %fq_subscription,
                        error = ?e,
                        "PubSub receive loop exited with error"
                    );
                } else {
                    tracing::warn!(
                        topic = %fq_topic,
                        subscription = %fq_subscription,
                        "PubSub receive loop exited normally"
                    );
                }
            });
        }
    }

    // Even in pull mode we still need the HTTP listener for Cloud Tasks wakeups
    // and internal MCP routes like talon-ops.
    let app = Router::new()
        .route("/pubsub/push", post(push_webhook))
        .route("/schedules/fire", post(schedule_fire))
        .nest(
            "/mcp/talon-ops",
            talon::worker::talon_ops::talon_ops_router(handler.clone()),
        )
        .with_state(handler);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8081".to_string());
    println!(
        "Worker listening for Push events / Health checks on 0.0.0.0:{}",
        port
    );
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;

    axum::serve(listener, app).await?;

    Ok(())
}
