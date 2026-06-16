// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::config::{proto::CloudTasksSchedulerConfig, SecretExt};
use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use chrono::{DateTime, Utc};
use google_cloud_auth::credentials::{AccessTokenCredentials, Builder as CredentialsBuilder};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use super::{ScheduleWakeupRequest, ScheduledWakeup, SchedulerBackend, SCHEDULER_AUTH_HEADER};

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const MAX_CLOUD_TASK_SCHEDULE_AHEAD_SECS: i64 = 30 * 24 * 60 * 60;
const CLOUD_TASKS_HTTP_TIMEOUT_SECS: u64 = 10;
const CLOUD_TASKS_API_BASE: &str = "https://cloudtasks.googleapis.com/v2";

pub struct CloudTasksSchedulerBackend {
    client: reqwest::Client,
    credentials: AccessTokenCredentials,
    parent: String,
    target_url: String,
    auth_token: Option<String>,
    oidc_service_account_email: Option<String>,
    oidc_audience: Option<String>,
    api_base: String,
}

impl CloudTasksSchedulerBackend {
    pub async fn new(cfg: &CloudTasksSchedulerConfig) -> Result<Self> {
        let project_id = non_empty(cfg.project_id.clone())
            .or_else(|| std::env::var("TALON_SCHEDULER_PROJECT_ID").ok())
            .or_else(|| std::env::var("GCP_PROJECT_ID").ok())
            .unwrap_or_default();
        let location = non_empty(cfg.location.clone())
            .or_else(|| std::env::var("TALON_SCHEDULER_LOCATION").ok())
            .unwrap_or_default();
        let queue = non_empty(cfg.queue.clone())
            .or_else(|| std::env::var("TALON_SCHEDULER_QUEUE").ok())
            .unwrap_or_default();
        let target_url = non_empty(cfg.target_url.clone())
            .or_else(|| std::env::var("TALON_SCHEDULER_TARGET_URL").ok())
            .unwrap_or_default();
        if project_id.is_empty() || location.is_empty() || queue.is_empty() || target_url.is_empty()
        {
            return Err(anyhow!(
                "cloud_tasks scheduler requires project_id, location, queue, and target_url"
            ));
        }

        let credentials = CredentialsBuilder::default()
            .with_scopes([CLOUD_PLATFORM_SCOPE])
            .build_access_token_credentials()
            .context("failed to build Google access token credentials for Cloud Tasks")?;

        let (auth_token, oidc_service_account_email, oidc_audience) =
            resolve_callback_auth(cfg.callback_auth.as_ref()).await?;

        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(
                    CLOUD_TASKS_HTTP_TIMEOUT_SECS,
                ))
                .build()
                .context("failed to build Cloud Tasks HTTP client")?,
            credentials,
            parent: format!(
                "projects/{}/locations/{}/queues/{}",
                project_id, location, queue
            ),
            target_url,
            auth_token,
            oidc_service_account_email,
            oidc_audience,
            api_base: CLOUD_TASKS_API_BASE.to_string(),
        })
    }

    async fn bearer_token(&self) -> Result<String> {
        Ok(self.credentials.access_token().await?.token)
    }
}

fn compute_cloud_tasks_schedule_at(
    now: DateTime<Utc>,
    fire_at: DateTime<Utc>,
) -> (DateTime<Utc>, bool) {
    let max_schedule_at = now + chrono::Duration::seconds(MAX_CLOUD_TASK_SCHEDULE_AHEAD_SECS - 60);
    let schedule_at = std::cmp::min(fire_at, max_schedule_at);
    (schedule_at, schedule_at != fire_at)
}

async fn resolve_callback_auth(
    cfg: Option<&crate::control::config::proto::SchedulerCallbackAuthConfig>,
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    if let Some(cfg) = cfg {
        return match cfg.auth.as_ref() {
            Some(
                crate::control::config::proto::scheduler_callback_auth_config::Auth::SharedSecret(
                    secret,
                ),
            ) => Ok((Some(secret.resolve().await?), None, None)),
            Some(
                crate::control::config::proto::scheduler_callback_auth_config::Auth::GoogleOidc(
                    oidc,
                ),
            ) => Ok((
                None,
                non_empty(oidc.service_account_email.clone()),
                non_empty(oidc.audience.clone()),
            )),
            None => Ok((None, None, None)),
        };
    }

    Ok((
        std::env::var("TALON_SCHEDULER_AUTH_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty()),
        std::env::var("TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL")
            .ok()
            .filter(|value| !value.trim().is_empty()),
        std::env::var("TALON_SCHEDULER_AUDIENCE")
            .ok()
            .filter(|value| !value.trim().is_empty()),
    ))
}

#[async_trait::async_trait]
impl SchedulerBackend for CloudTasksSchedulerBackend {
    async fn schedule(&self, req: ScheduleWakeupRequest) -> Result<ScheduledWakeup> {
        let now = Utc::now();
        let (schedule_at, is_checkpoint) = compute_cloud_tasks_schedule_at(now, req.fire_at);
        if is_checkpoint {
            tracing::info!(
                schedule_id = %req.schedule_id,
                schedule_at = %schedule_at,
                fire_at = %req.fire_at,
                "Scheduling Cloud Tasks checkpoint before the final intended fire time"
            );
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct OidcToken<'a> {
            service_account_email: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            audience: Option<&'a str>,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct HttpRequest<'a> {
            http_method: &'static str,
            url: &'a str,
            headers: std::collections::HashMap<&'static str, String>,
            body: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            oidc_token: Option<OidcToken<'a>>,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Task<'a> {
            schedule_time: String,
            http_request: HttpRequest<'a>,
        }

        #[derive(Serialize)]
        struct CreateTaskRequest<'a> {
            task: Task<'a>,
        }

        #[derive(Deserialize)]
        struct TaskResponse {
            name: Option<String>,
        }

        let mut headers = std::collections::HashMap::new();
        headers.insert("Content-Type", "application/json".to_string());
        if let Some(token) = &self.auth_token {
            headers.insert(SCHEDULER_AUTH_HEADER, token.clone());
        }

        let body = base64::engine::general_purpose::STANDARD.encode(req.payload);
        let oidc_token = self
            .oidc_service_account_email
            .as_deref()
            .map(|service_account_email| OidcToken {
                service_account_email,
                audience: self.oidc_audience.as_deref(),
            });
        let create_req = CreateTaskRequest {
            task: Task {
                schedule_time: schedule_at.to_rfc3339(),
                http_request: HttpRequest {
                    http_method: "POST",
                    url: &self.target_url,
                    headers,
                    body,
                    oidc_token,
                },
            },
        };

        let url = format!("{}/{}/tasks", self.api_base, self.parent);
        let resp = self
            .client
            .post(url)
            .bearer_auth(self.bearer_token().await?)
            .json(&create_req)
            .send()
            .await
            .context("failed to create Cloud Task")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "cloud tasks create failed: status={} body={}",
                status,
                body
            ));
        }

        let parsed: TaskResponse = resp
            .json()
            .await
            .context("failed to parse Cloud Tasks response")?;
        Ok(ScheduledWakeup {
            handle: parsed.name,
            armed: true,
        })
    }

    async fn cancel(&self, handle: &str) -> Result<()> {
        let resp = self
            .client
            .delete(format!("{}/{}", self.api_base, handle))
            .bearer_auth(self.bearer_token().await?)
            .send()
            .await
            .context("failed to delete Cloud Task")?;

        if resp.status().is_success() || resp.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(anyhow!(
            "cloud tasks delete failed: status={} body={}",
            status,
            body
        ))
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path, State};
    use axum::routing::{delete, post};
    use axum::{Json, Router};
    use chrono::TimeZone;
    use google_cloud_auth::credentials::{
        AccessToken, AccessTokenCredentialsProvider, CacheableResource, CredentialsProvider,
        EntityTag,
    };
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    #[derive(Clone, Debug)]
    struct StaticAccessTokenCredentials;

    impl CredentialsProvider for StaticAccessTokenCredentials {
        async fn headers(
            &self,
            _extensions: axum::http::Extensions,
        ) -> std::result::Result<
            CacheableResource<axum::http::HeaderMap>,
            google_cloud_auth::errors::CredentialsError,
        > {
            Ok(CacheableResource::New {
                data: axum::http::HeaderMap::new(),
                entity_tag: EntityTag::default(),
            })
        }

        async fn universe_domain(&self) -> Option<String> {
            None
        }
    }

    impl AccessTokenCredentialsProvider for StaticAccessTokenCredentials {
        async fn access_token(
            &self,
        ) -> std::result::Result<AccessToken, google_cloud_auth::errors::CredentialsError> {
            Ok(AccessToken {
                token: "cloud-tasks-token".to_string(),
            })
        }
    }

    impl CloudTasksSchedulerBackend {
        #[cfg(test)]
        fn for_tests(
            api_base: String,
            target_url: String,
            auth_token: Option<String>,
            oidc_service_account_email: Option<String>,
            oidc_audience: Option<String>,
        ) -> Self {
            Self {
                client: reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(
                        CLOUD_TASKS_HTTP_TIMEOUT_SECS,
                    ))
                    .build()
                    .unwrap(),
                credentials: AccessTokenCredentials::from(StaticAccessTokenCredentials),
                parent: "projects/test/locations/us-central1/queues/talon".to_string(),
                target_url,
                auth_token,
                oidc_service_account_email,
                oidc_audience,
                api_base,
            }
        }
    }

    #[derive(Clone, Default)]
    struct RequestCapture {
        auth_header: Arc<Mutex<Option<String>>>,
        scheduler_header: Arc<Mutex<Option<String>>>,
        oidc_email: Arc<Mutex<Option<String>>>,
        oidc_audience: Arc<Mutex<Option<String>>>,
        request_url: Arc<Mutex<Option<String>>>,
    }

    fn clear_scheduler_auth_env() {
        unsafe {
            std::env::remove_var("TALON_SCHEDULER_AUTH_TOKEN");
            std::env::remove_var("TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL");
            std::env::remove_var("TALON_SCHEDULER_AUDIENCE");
        }
    }

    #[test]
    fn cloud_tasks_schedule_at_uses_final_fire_when_within_horizon() {
        let now = Utc.with_ymd_and_hms(2026, 5, 4, 12, 0, 0).unwrap();
        let fire_at = now + chrono::Duration::days(7);

        let (schedule_at, is_checkpoint) = compute_cloud_tasks_schedule_at(now, fire_at);

        assert_eq!(schedule_at, fire_at);
        assert!(!is_checkpoint);
    }

    #[test]
    fn cloud_tasks_schedule_at_creates_checkpoint_before_far_future_fire() {
        let now = Utc.with_ymd_and_hms(2026, 5, 4, 12, 0, 0).unwrap();
        let fire_at = now + chrono::Duration::days(45);

        let (schedule_at, is_checkpoint) = compute_cloud_tasks_schedule_at(now, fire_at);

        assert_eq!(
            schedule_at,
            now + chrono::Duration::seconds(MAX_CLOUD_TASK_SCHEDULE_AHEAD_SECS - 60)
        );
        assert!(is_checkpoint);
        assert!(schedule_at < fire_at);
    }

    #[test]
    fn non_empty_trims_and_discards_blank_values() {
        assert_eq!(non_empty(" value ".to_string()).as_deref(), Some("value"));
        assert!(non_empty("   ".to_string()).is_none());
        assert!(non_empty(String::new()).is_none());
    }

    #[tokio::test]
    async fn resolve_callback_auth_prefers_explicit_google_oidc_over_env_secret() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        clear_scheduler_auth_env();
        unsafe {
            std::env::set_var("TALON_SCHEDULER_AUTH_TOKEN", "stale-secret");
            std::env::set_var("TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL", "stale@example.com");
            std::env::set_var("TALON_SCHEDULER_AUDIENCE", "https://stale.example.com");
        }

        let cfg = crate::control::config::proto::SchedulerCallbackAuthConfig {
            auth: Some(
                crate::control::config::proto::scheduler_callback_auth_config::Auth::GoogleOidc(
                    crate::control::config::proto::GoogleOidcAuthConfig {
                        service_account_email: "scheduler@example.com".to_string(),
                        audience: "https://worker.example.com".to_string(),
                    },
                ),
            ),
        };

        let (auth_token, service_account_email, audience) =
            resolve_callback_auth(Some(&cfg)).await.unwrap();

        assert_eq!(auth_token, None);
        assert_eq!(
            service_account_email.as_deref(),
            Some("scheduler@example.com")
        );
        assert_eq!(audience.as_deref(), Some("https://worker.example.com"));

        clear_scheduler_auth_env();
    }

    #[tokio::test]
    async fn resolve_callback_auth_uses_env_when_config_absent() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        clear_scheduler_auth_env();
        unsafe {
            std::env::set_var("TALON_SCHEDULER_AUTH_TOKEN", "shared-secret");
            std::env::set_var(
                "TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL",
                "scheduler@example.com",
            );
            std::env::set_var("TALON_SCHEDULER_AUDIENCE", "https://worker.example.com");
        }

        let (auth_token, service_account_email, audience) =
            resolve_callback_auth(None).await.unwrap();

        assert_eq!(auth_token.as_deref(), Some("shared-secret"));
        assert_eq!(
            service_account_email.as_deref(),
            Some("scheduler@example.com")
        );
        assert_eq!(audience.as_deref(), Some("https://worker.example.com"));

        clear_scheduler_auth_env();
    }

    #[tokio::test]
    async fn resolve_callback_auth_handles_blank_and_shared_secret_config() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        clear_scheduler_auth_env();

        let blank_cfg = crate::control::config::proto::SchedulerCallbackAuthConfig {
            auth: Some(
                crate::control::config::proto::scheduler_callback_auth_config::Auth::GoogleOidc(
                    crate::control::config::proto::GoogleOidcAuthConfig {
                        service_account_email: "   ".to_string(),
                        audience: "".to_string(),
                    },
                ),
            ),
        };
        let (auth_token, service_account_email, audience) =
            resolve_callback_auth(Some(&blank_cfg)).await.unwrap();
        assert!(auth_token.is_none());
        assert!(service_account_email.is_none());
        assert!(audience.is_none());

        let shared_cfg = crate::control::config::proto::SchedulerCallbackAuthConfig {
            auth: Some(
                crate::control::config::proto::scheduler_callback_auth_config::Auth::SharedSecret(
                    crate::control::config::proto::Secret {
                        source: Some(crate::control::config::proto::secret::Source::Ref(
                            crate::control::config::proto::SecretRef {
                                source: crate::control::config::proto::secret_ref::Source::Env
                                    as i32,
                                key: "TALON_SCHEDULER_AUTH_TOKEN".to_string(),
                            },
                        )),
                    },
                ),
            ),
        };
        unsafe {
            std::env::set_var("TALON_SCHEDULER_AUTH_TOKEN", "shared-secret");
        }
        let (auth_token, service_account_email, audience) =
            resolve_callback_auth(Some(&shared_cfg)).await.unwrap();
        assert_eq!(auth_token.as_deref(), Some("shared-secret"));
        assert!(service_account_email.is_none());
        assert!(audience.is_none());

        clear_scheduler_auth_env();
    }

    #[tokio::test]
    async fn new_requires_scheduler_fields_and_reads_env_fallbacks() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        clear_scheduler_auth_env();
        unsafe {
            std::env::remove_var("TALON_SCHEDULER_PROJECT_ID");
            std::env::remove_var("TALON_SCHEDULER_LOCATION");
            std::env::remove_var("TALON_SCHEDULER_QUEUE");
            std::env::remove_var("TALON_SCHEDULER_TARGET_URL");
            std::env::remove_var("GCP_PROJECT_ID");
        }

        let err = CloudTasksSchedulerBackend::new(&CloudTasksSchedulerConfig::default())
            .await
            .err()
            .expect("missing config should fail");
        assert!(err
            .to_string()
            .contains("requires project_id, location, queue, and target_url"));

        unsafe {
            std::env::set_var("GCP_PROJECT_ID", "env-project");
            std::env::set_var("TALON_SCHEDULER_LOCATION", "us-central1");
            std::env::set_var("TALON_SCHEDULER_QUEUE", "jobs");
            std::env::set_var(
                "TALON_SCHEDULER_TARGET_URL",
                "https://worker.example.com/fire",
            );
            std::env::set_var("TALON_SCHEDULER_AUTH_TOKEN", "env-secret");
            std::env::set_var(
                "TALON_SCHEDULER_SERVICE_ACCOUNT_EMAIL",
                "scheduler@example.com",
            );
            std::env::set_var(
                "TALON_SCHEDULER_AUDIENCE",
                "https://worker.example.com/fire",
            );
        }

        let backend = CloudTasksSchedulerBackend::new(&CloudTasksSchedulerConfig::default())
            .await
            .expect("backend should build from env");
        assert_eq!(
            backend.parent,
            "projects/env-project/locations/us-central1/queues/jobs"
        );
        assert_eq!(backend.target_url, "https://worker.example.com/fire");
        assert_eq!(backend.auth_token.as_deref(), Some("env-secret"));
        assert_eq!(
            backend.oidc_service_account_email.as_deref(),
            Some("scheduler@example.com")
        );
        assert_eq!(
            backend.oidc_audience.as_deref(),
            Some("https://worker.example.com/fire")
        );

        clear_scheduler_auth_env();
        unsafe {
            std::env::remove_var("TALON_SCHEDULER_PROJECT_ID");
            std::env::remove_var("TALON_SCHEDULER_LOCATION");
            std::env::remove_var("TALON_SCHEDULER_QUEUE");
            std::env::remove_var("TALON_SCHEDULER_TARGET_URL");
            std::env::remove_var("GCP_PROJECT_ID");
        }
    }

    #[tokio::test]
    async fn new_prefers_trimmed_config_values_over_env() {
        let _guard = crate::test_support::async_env_mutex().lock().await;
        clear_scheduler_auth_env();
        unsafe {
            std::env::set_var("GCP_PROJECT_ID", "env-project");
            std::env::set_var("TALON_SCHEDULER_LOCATION", "env-location");
            std::env::set_var("TALON_SCHEDULER_QUEUE", "env-queue");
            std::env::set_var("TALON_SCHEDULER_TARGET_URL", "https://env.example.com/fire");
            std::env::set_var("TALON_SCHEDULER_AUTH_TOKEN", "env-secret");
        }

        let backend = CloudTasksSchedulerBackend::new(&CloudTasksSchedulerConfig {
            project_id: " cfg-project ".to_string(),
            location: " us-west1 ".to_string(),
            queue: " main ".to_string(),
            target_url: " https://cfg.example.com/fire ".to_string(),
            callback_auth: Some(crate::control::config::proto::SchedulerCallbackAuthConfig {
                auth: Some(
                    crate::control::config::proto::scheduler_callback_auth_config::Auth::SharedSecret(
                        crate::control::config::proto::Secret {
                            source: Some(crate::control::config::proto::secret::Source::Ref(
                                crate::control::config::proto::SecretRef {
                                    source: crate::control::config::proto::secret_ref::Source::Env as i32,
                                    key: "TALON_SCHEDULER_AUTH_TOKEN".to_string(),
                                },
                            )),
                        },
                    ),
                ),
            }),
        })
        .await
        .expect("backend should build from config");

        assert_eq!(
            backend.parent,
            "projects/cfg-project/locations/us-west1/queues/main"
        );
        assert_eq!(backend.target_url, "https://cfg.example.com/fire");
        assert_eq!(backend.auth_token.as_deref(), Some("env-secret"));

        clear_scheduler_auth_env();
        unsafe {
            std::env::remove_var("GCP_PROJECT_ID");
            std::env::remove_var("TALON_SCHEDULER_LOCATION");
            std::env::remove_var("TALON_SCHEDULER_QUEUE");
            std::env::remove_var("TALON_SCHEDULER_TARGET_URL");
        }
    }

    #[tokio::test]
    async fn schedule_and_cancel_cloud_task_cover_success_and_error_paths() {
        let capture = RequestCapture::default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/v2/projects/test/locations/us-central1/queues/talon/tasks",
                post({
                    move |State(capture): State<RequestCapture>,
                          headers: axum::http::HeaderMap,
                          Json(body): Json<serde_json::Value>| async move {
                        *capture.auth_header.lock().await = headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string);
                        *capture.scheduler_header.lock().await = body
                            .pointer("/task/httpRequest/headers/X-Talon-Scheduler-Token")
                            .and_then(|value| value.as_str())
                            .map(str::to_string);
                        *capture.oidc_email.lock().await = body
                            .pointer("/task/httpRequest/oidcToken/serviceAccountEmail")
                            .and_then(|value| value.as_str())
                            .map(str::to_string);
                        *capture.oidc_audience.lock().await = body
                            .pointer("/task/httpRequest/oidcToken/audience")
                            .and_then(|value| value.as_str())
                            .map(str::to_string);
                        *capture.request_url.lock().await = body
                            .pointer("/task/httpRequest/url")
                            .and_then(|value| value.as_str())
                            .map(str::to_string);

                        if body
                            .pointer("/task/httpRequest/body")
                            .and_then(|value| value.as_str())
                            == Some("ZXJyb3I=")
                        {
                            return (
                                axum::http::StatusCode::BAD_REQUEST,
                                Json(serde_json::json!({"error": "bad task"})),
                            );
                        }

                        (
                            axum::http::StatusCode::OK,
                            Json(serde_json::json!({"name": "task-123"})),
                        )
                    }
                }),
            )
            .route(
                "/v2/:handle",
                delete(
                    |Path(handle): Path<String>,
                     State(_capture): State<RequestCapture>| async move {
                        if handle == "missing" {
                            return axum::http::StatusCode::NOT_FOUND;
                        }
                        if handle == "boom" {
                            return axum::http::StatusCode::INTERNAL_SERVER_ERROR;
                        }
                        axum::http::StatusCode::OK
                    },
                ),
            )
            .with_state(capture.clone());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let backend = CloudTasksSchedulerBackend::for_tests(
            format!("http://{}/v2", addr),
            "https://worker.example.com/schedules/fire".to_string(),
            Some("scheduler-secret".to_string()),
            Some("scheduler@example.com".to_string()),
            Some("https://worker.example.com/schedules/fire".to_string()),
        );

        let scheduled = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "default".to_string(),
                schedule_id: "nightly".to_string(),
                revision: 4,
                fire_at: Utc::now() + chrono::Duration::minutes(5),
                payload: b"hello".to_vec(),
            })
            .await
            .unwrap();
        assert_eq!(scheduled.handle.as_deref(), Some("task-123"));
        assert!(scheduled.armed);
        assert_eq!(
            capture.auth_header.lock().await.as_deref(),
            Some("Bearer cloud-tasks-token")
        );
        assert_eq!(
            capture.scheduler_header.lock().await.as_deref(),
            Some("scheduler-secret")
        );
        assert_eq!(
            capture.oidc_email.lock().await.as_deref(),
            Some("scheduler@example.com")
        );
        assert_eq!(
            capture.oidc_audience.lock().await.as_deref(),
            Some("https://worker.example.com/schedules/fire")
        );
        assert_eq!(
            capture.request_url.lock().await.as_deref(),
            Some("https://worker.example.com/schedules/fire")
        );

        let schedule_err = backend
            .schedule(ScheduleWakeupRequest {
                namespace: "default".to_string(),
                schedule_id: "nightly".to_string(),
                revision: 4,
                fire_at: Utc::now() + chrono::Duration::minutes(5),
                payload: b"error".to_vec(),
            })
            .await
            .unwrap_err();
        assert!(schedule_err
            .to_string()
            .contains("cloud tasks create failed: status=400 Bad Request"));

        backend.cancel("existing").await.unwrap();
        backend.cancel("missing").await.unwrap();
        let cancel_err = backend.cancel("boom").await.unwrap_err();
        assert!(cancel_err
            .to_string()
            .contains("cloud tasks delete failed: status=500 Internal Server Error"));
    }
}
