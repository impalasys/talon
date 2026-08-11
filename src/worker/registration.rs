// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{ns, ControlPlane};
use crate::gateway::rpc::resources_proto;
use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;
use std::net::Ipv4Addr;
use std::sync::{Arc, OnceLock};
use tokio_util::sync::CancellationToken;
use url::Url;

pub const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
pub const HEARTBEAT_TTL: chrono::Duration = chrono::Duration::seconds(30);
pub const CLOUD_RUN_WORKER_POOL_DISCOVERY: &str = "cloud_run_worker_pool";

const CLOUD_RUN_METADATA_URL: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/network-interfaces/0/ip";
const CLOUD_RUN_METADATA_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(750);
const CLOUD_RUN_METADATA_MAX_ATTEMPTS: usize = 3;

static GENERATED_WORKER_ID: OnceLock<String> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct WorkerRegistration {
    pub worker_id: String,
    pub started_at: i64,
    pub version: String,
    pub endpoints: Vec<resources_proto::WorkerEndpoint>,
}

impl WorkerRegistration {
    pub fn new(worker_id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            worker_id: worker_id.into(),
            started_at: chrono::Utc::now().timestamp_micros(),
            version: version.into(),
            endpoints: Vec::new(),
        }
    }

    pub fn with_endpoints(mut self, endpoints: Vec<resources_proto::WorkerEndpoint>) -> Self {
        self.endpoints = endpoints;
        self
    }
}

pub fn worker_id() -> String {
    GENERATED_WORKER_ID
        .get_or_init(crate::control::uuid::worker_id)
        .clone()
}

pub async fn upsert_worker(cp: &ControlPlane, registration: &WorkerRegistration) -> Result<()> {
    let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    store
        .upsert_manifest(
            ns::TALON_SYSTEM,
            resources_proto::ResourceManifest {
                api_version: "talon.impalasys.com/v1".to_string(),
                kind: "Worker".to_string(),
                metadata: Some(resources_proto::ResourceMeta {
                    name: registration.worker_id.clone(),
                    namespace: ns::TALON_SYSTEM.to_string(),
                    ..Default::default()
                }),
                spec: Some(resources_proto::ResourceSpec {
                    kind: Some(resources_proto::resource_spec::Kind::Worker(
                        resources_proto::WorkerSpec {},
                    )),
                }),
            },
        )
        .await
        .with_context(|| format!("failed to upsert Worker '{}'", registration.worker_id))?;
    tracing::info!(worker_id = %registration.worker_id, "Worker registered");
    Ok(())
}

pub async fn patch_worker_status(
    cp: &ControlPlane,
    registration: &WorkerRegistration,
    phase: &str,
) -> Result<()> {
    let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
    store
        .patch_status(
            ns::TALON_SYSTEM,
            "Worker",
            &registration.worker_id,
            None,
            resources_proto::ResourceStatus {
                kind: Some(resources_proto::resource_status::Kind::Worker(
                    worker_status(registration, phase),
                )),
            },
        )
        .await
        .with_context(|| format!("failed to patch Worker '{}' status", registration.worker_id))?;
    Ok(())
}

pub fn worker_status(
    registration: &WorkerRegistration,
    phase: &str,
) -> resources_proto::WorkerStatus {
    let now = chrono::Utc::now();
    resources_proto::WorkerStatus {
        observed_generation: 0,
        phase: phase.to_string(),
        conditions: Vec::new(),
        started_at: registration.started_at,
        heartbeat_at: now.timestamp_micros(),
        expires_at: (now + HEARTBEAT_TTL).timestamp_micros(),
        version: registration.version.clone(),
        endpoints: if phase == "ready" {
            registration.endpoints.clone()
        } else {
            Vec::new()
        },
    }
}

#[async_trait::async_trait]
pub trait MetadataClient: Send + Sync {
    async fn get(&self, url: &str, headers: HeaderMap) -> Result<String>;
}

struct HttpMetadataClient;

#[async_trait::async_trait]
impl MetadataClient for HttpMetadataClient {
    async fn get(&self, url: &str, headers: HeaderMap) -> Result<String> {
        let client = reqwest::Client::builder()
            .timeout(CLOUD_RUN_METADATA_TIMEOUT)
            .build()
            .context("failed to build metadata client")?;
        let response = client
            .get(url)
            .headers(headers)
            .send()
            .await
            .context("metadata request failed")?;
        if !response.status().is_success() {
            anyhow::bail!("metadata server returned HTTP {}", response.status());
        }
        response
            .text()
            .await
            .context("failed to read metadata response")
    }
}

pub fn cloud_run_worker_pool_discovery_enabled<F>(get: &F) -> bool
where
    F: Fn(&str) -> Option<String>,
{
    get("TALON_WORKER_ENDPOINT_DISCOVERY")
        .is_some_and(|value| value.trim() == CLOUD_RUN_WORKER_POOL_DISCOVERY)
}

pub async fn discover_worker_endpoints<F>(
    get: F,
    port: &str,
) -> Result<Vec<resources_proto::WorkerEndpoint>>
where
    F: Fn(&str) -> Option<String>,
{
    let metadata_client = HttpMetadataClient;
    discover_worker_endpoints_with_metadata_client(get, port, &metadata_client).await
}

pub async fn discover_worker_endpoints_with_metadata_client<F>(
    get: F,
    port: &str,
    metadata_client: &dyn MetadataClient,
) -> Result<Vec<resources_proto::WorkerEndpoint>>
where
    F: Fn(&str) -> Option<String>,
{
    if cloud_run_worker_pool_discovery_enabled(&get) {
        if let Some(endpoint) = talon_explicit_worker_endpoint(&get) {
            return Ok(vec![endpoint]);
        }

        let endpoints: Vec<_> = cloud_run_worker_endpoint(&get, port, metadata_client)
            .await
            .into_iter()
            .collect();
        if endpoints.is_empty() {
            anyhow::bail!(
                "Cloud Run Worker Pool endpoint discovery produced no usable private IP; verify Direct VPC ingress, metadata access, and gateway VPC reachability"
            );
        }
        return Ok(endpoints);
    }

    if let Some(endpoint) = explicit_worker_endpoint(&get) {
        return Ok(vec![endpoint]);
    }

    if let Some(endpoint) = ecs_worker_endpoint(&get, port).await {
        return Ok(vec![endpoint]);
    }

    Ok(Vec::new())
}

fn talon_explicit_worker_endpoint<F>(get: &F) -> Option<resources_proto::WorkerEndpoint>
where
    F: Fn(&str) -> Option<String>,
{
    [
        "TALON_WORKER_ENDPOINT_URL",
        "TALON_WORKER_PUBLIC_URL",
        "TALON_WORKER_URL",
    ]
    .into_iter()
    .find_map(|name| get(name))
    .and_then(|url| worker_endpoint_from_url(&url, get))
}

fn explicit_worker_endpoint<F>(get: &F) -> Option<resources_proto::WorkerEndpoint>
where
    F: Fn(&str) -> Option<String>,
{
    [
        "TALON_WORKER_ENDPOINT_URL",
        "TALON_WORKER_PUBLIC_URL",
        "TALON_WORKER_URL",
        "CLOUD_RUN_SERVICE_URL",
    ]
    .into_iter()
    .find_map(|name| get(name))
    .and_then(|url| worker_endpoint_from_url(&url, get))
}

fn worker_endpoint_from_url<F>(raw_url: &str, get: &F) -> Option<resources_proto::WorkerEndpoint>
where
    F: Fn(&str) -> Option<String>,
{
    let url = raw_url.trim().trim_end_matches('/');
    let parsed = Url::parse(url).ok()?;
    if url.is_empty() {
        return None;
    }
    let default_protocol = if parsed.scheme() == "unix" {
        "grpc"
    } else {
        "http"
    };
    Some(resources_proto::WorkerEndpoint {
        url: url.to_string(),
        protocol: get("TALON_WORKER_ENDPOINT_PROTOCOL")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| default_protocol.to_string()),
        audience: get("TALON_WORKER_ENDPOINT_AUDIENCE").unwrap_or_default(),
    })
}

async fn ecs_worker_endpoint<F>(get: &F, port: &str) -> Option<resources_proto::WorkerEndpoint>
where
    F: Fn(&str) -> Option<String>,
{
    let metadata_uri = get("ECS_CONTAINER_METADATA_URI_V4")?;
    let metadata = fetch_json_metadata(&metadata_uri).await?;
    let address = first_ecs_ipv4_address(&metadata)?;
    worker_endpoint_from_url(&format!("http://{}:{}", address, port), get)
}

async fn cloud_run_worker_endpoint<F>(
    get: &F,
    port: &str,
    metadata_client: &dyn MetadataClient,
) -> Option<resources_proto::WorkerEndpoint>
where
    F: Fn(&str) -> Option<String>,
{
    for attempt in 1..=CLOUD_RUN_METADATA_MAX_ATTEMPTS {
        match fetch_cloud_run_ipv4(metadata_client).await {
            Ok(address) => {
                return worker_endpoint_from_url(&format!("http://{}:{}", address, port), get);
            }
            Err(err) if attempt < CLOUD_RUN_METADATA_MAX_ATTEMPTS => {
                tracing::warn!(
                    attempt,
                    max_attempts = CLOUD_RUN_METADATA_MAX_ATTEMPTS,
                    error = %err,
                    "Cloud Run Worker Pool metadata discovery failed; retrying"
                );
                tokio::time::sleep(std::time::Duration::from_millis(
                    100 * 2u64.pow((attempt - 1) as u32),
                ))
                .await;
            }
            Err(err) => {
                tracing::error!(
                    attempt,
                    error = %err,
                    "Cloud Run Worker Pool metadata discovery failed; worker will not be marked ready"
                );
                return None;
            }
        }
    }
    None
}

async fn fetch_cloud_run_ipv4(metadata_client: &dyn MetadataClient) -> Result<Ipv4Addr> {
    let mut headers = HeaderMap::new();
    headers.insert("Metadata-Flavor", HeaderValue::from_static("Google"));
    let raw_address = metadata_client.get(CLOUD_RUN_METADATA_URL, headers).await?;
    let address = raw_address
        .trim()
        .parse::<Ipv4Addr>()
        .context("metadata response was not a valid IPv4 address")?;
    if !address.is_private() || address.is_loopback() {
        anyhow::bail!("metadata response was not a private IPv4 address");
    }
    Ok(address)
}

async fn fetch_json_metadata(url: &str) -> Option<Value> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(750))
        .build()
        .ok()?;
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<Value>().await.ok()
}

fn first_ecs_ipv4_address(metadata: &Value) -> Option<&str> {
    metadata
        .get("Networks")?
        .as_array()?
        .iter()
        .flat_map(|network| {
            network
                .get("IPv4Addresses")
                .and_then(|addresses| addresses.as_array())
                .into_iter()
                .flatten()
        })
        .filter_map(|address| address.as_str())
        .find(|address| !address.trim().is_empty())
}

pub fn worker_is_live(status: &resources_proto::WorkerStatus, now_micros: i64) -> bool {
    status.expires_at > now_micros
}

pub fn worker_is_stale(status: &resources_proto::WorkerStatus, now_micros: i64) -> bool {
    !worker_is_live(status, now_micros)
}

pub async fn run_worker_heartbeat(
    cp: Arc<ControlPlane>,
    registration: WorkerRegistration,
    shutdown_token: CancellationToken,
) {
    register_and_patch_ready(cp.as_ref(), &registration).await;

    loop {
        tokio::select! {
            _ = shutdown_token.cancelled() => break,
            _ = tokio::time::sleep(HEARTBEAT_INTERVAL) => {
                patch_ready_with_registration_retry(cp.as_ref(), &registration).await;
            }
        }
    }

    if let Err(err) = patch_worker_status(cp.as_ref(), &registration, "draining").await {
        tracing::warn!(worker_id = %registration.worker_id, error = %err, "Worker draining status update failed");
    }
}

async fn register_and_patch_ready(cp: &ControlPlane, registration: &WorkerRegistration) {
    if let Err(err) = upsert_worker(cp, registration).await {
        tracing::warn!(worker_id = %registration.worker_id, error = %err, "Worker registration failed");
        return;
    }

    if let Err(err) = patch_worker_status(cp, registration, "ready").await {
        tracing::warn!(worker_id = %registration.worker_id, error = %err, "Worker heartbeat failed");
    }
}

async fn patch_ready_with_registration_retry(cp: &ControlPlane, registration: &WorkerRegistration) {
    match patch_worker_status(cp, registration, "ready").await {
        Ok(()) => return,
        Err(err) => {
            tracing::warn!(worker_id = %registration.worker_id, error = %err, "Worker heartbeat failed");
        }
    }

    if let Err(err) = upsert_worker(cp, registration).await {
        tracing::warn!(worker_id = %registration.worker_id, error = %err, "Worker registration failed");
        return;
    }

    if let Err(err) = patch_worker_status(cp, registration, "ready").await {
        tracing::warn!(worker_id = %registration.worker_id, error = %err, "Worker heartbeat retry failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{EmptyPubSub, MockKvStore};
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MockMetadataClient {
        response: MockMetadataResponse,
        requests: Mutex<Vec<(String, Option<String>)>>,
    }

    enum MockMetadataResponse {
        Success(String),
        Failure,
    }

    impl MockMetadataClient {
        fn success(response: &str) -> Self {
            Self {
                response: MockMetadataResponse::Success(response.to_string()),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn failure() -> Self {
            Self {
                response: MockMetadataResponse::Failure,
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl MetadataClient for MockMetadataClient {
        async fn get(&self, url: &str, headers: HeaderMap) -> Result<String> {
            self.requests.lock().unwrap().push((
                url.to_string(),
                headers
                    .get("Metadata-Flavor")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string),
            ));
            match &self.response {
                MockMetadataResponse::Success(response) => Ok(response.clone()),
                MockMetadataResponse::Failure => anyhow::bail!("metadata request timed out"),
            }
        }
    }

    fn env(values: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let values: HashMap<String, String> = values
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
        move |name| values.get(name).cloned()
    }

    fn control_plane() -> ControlPlane {
        ControlPlane::builder(Arc::new(MockKvStore::default()), Arc::new(EmptyPubSub)).build()
    }

    #[test]
    fn worker_id_generates_stable_process_uuid() {
        let generated = worker_id();
        assert!(uuid::Uuid::parse_str(&generated).is_ok());
        assert_eq!(worker_id(), generated);
    }

    #[tokio::test]
    async fn worker_status_patch_preserves_spec_generation() {
        let cp = control_plane();
        let registration = WorkerRegistration::new("worker-a", "1.2.3");
        upsert_worker(&cp, &registration).await.unwrap();
        patch_worker_status(&cp, &registration, "ready")
            .await
            .unwrap();

        let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
        let worker = store
            .get(ns::TALON_SYSTEM, "Worker", "worker-a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(worker.metadata.as_ref().unwrap().generation, 1);
        assert!(matches!(
            worker.spec.as_ref().and_then(|spec| spec.kind.as_ref()),
            Some(resources_proto::resource_spec::Kind::Worker(_))
        ));
        let Some(resources_proto::resource_status::Kind::Worker(status)) =
            worker.status.and_then(|status| status.kind)
        else {
            panic!("expected Worker status");
        };
        assert_eq!(status.phase, "ready");
        assert!(status.heartbeat_at > 0);
        assert!(status.expires_at > status.heartbeat_at);
        assert_eq!(status.version, "1.2.3");
        assert!(status.endpoints.is_empty());
    }

    #[test]
    fn draining_status_clears_endpoints() {
        let registration = WorkerRegistration::new("worker-a", "1.2.3").with_endpoints(vec![
            resources_proto::WorkerEndpoint {
                url: "https://worker.example.com".to_string(),
                protocol: "http".to_string(),
                audience: "talon".to_string(),
            },
        ]);
        let ready = worker_status(&registration, "ready");
        assert_eq!(ready.endpoints.len(), 1);
        let draining = worker_status(&registration, "draining");
        assert_eq!(draining.phase, "draining");
        assert!(draining.endpoints.is_empty());
    }

    #[tokio::test]
    async fn worker_endpoint_discovery_prefers_explicit_url() {
        let endpoints = discover_worker_endpoints(
            |name| match name {
                "TALON_WORKER_ENDPOINT_URL" => Some("https://worker.example.com/".to_string()),
                "TALON_WORKER_ENDPOINT_AUDIENCE" => Some("scheduler".to_string()),
                _ => None,
            },
            "8081",
        )
        .await;
        let endpoints = endpoints.unwrap();

        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].url, "https://worker.example.com");
        assert_eq!(endpoints[0].protocol, "http");
        assert_eq!(endpoints[0].audience, "scheduler");
    }

    #[tokio::test]
    async fn worker_endpoint_discovery_accepts_unix_socket_urls() {
        let endpoints = discover_worker_endpoints(
            |name| match name {
                "TALON_WORKER_ENDPOINT_URL" => Some("unix:///tmp/talon-worker.sock".to_string()),
                _ => None,
            },
            "8081",
        )
        .await;
        let endpoints = endpoints.unwrap();

        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].url, "unix:///tmp/talon-worker.sock");
        assert_eq!(endpoints[0].protocol, "grpc");
    }

    #[tokio::test]
    async fn cloud_run_worker_pool_discovery_registers_private_ipv4_and_port() {
        let metadata = MockMetadataClient::success("10.0.0.15\n");
        let endpoints = discover_worker_endpoints_with_metadata_client(
            env(&[(
                "TALON_WORKER_ENDPOINT_DISCOVERY",
                CLOUD_RUN_WORKER_POOL_DISCOVERY,
            )]),
            "9090",
            &metadata,
        )
        .await
        .unwrap();

        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].url, "http://10.0.0.15:9090");
        assert_eq!(endpoints[0].protocol, "http");
        assert_eq!(
            metadata.requests.lock().unwrap().as_slice(),
            &[(
                CLOUD_RUN_METADATA_URL.to_string(),
                Some("Google".to_string())
            )]
        );
    }

    #[tokio::test]
    async fn cloud_run_worker_pool_discovery_rejects_invalid_or_empty_metadata() {
        for response in ["", "not-an-ip", "2001:db8::1"] {
            let metadata = MockMetadataClient::success(response);
            let result = discover_worker_endpoints_with_metadata_client(
                env(&[(
                    "TALON_WORKER_ENDPOINT_DISCOVERY",
                    CLOUD_RUN_WORKER_POOL_DISCOVERY,
                )]),
                "8081",
                &metadata,
            )
            .await;
            assert!(result.is_err(), "unexpected endpoint for {response:?}");
        }
    }

    #[tokio::test]
    async fn cloud_run_worker_pool_discovery_retries_metadata_failure() {
        let metadata = MockMetadataClient::failure();
        let result = discover_worker_endpoints_with_metadata_client(
            env(&[(
                "TALON_WORKER_ENDPOINT_DISCOVERY",
                CLOUD_RUN_WORKER_POOL_DISCOVERY,
            )]),
            "8081",
            &metadata,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(
            metadata.requests.lock().unwrap().len(),
            CLOUD_RUN_METADATA_MAX_ATTEMPTS
        );
    }

    #[tokio::test]
    async fn cloud_run_worker_pool_discovery_rejects_localhost() {
        let metadata = MockMetadataClient::success("127.0.0.1");
        let result = discover_worker_endpoints_with_metadata_client(
            env(&[(
                "TALON_WORKER_ENDPOINT_DISCOVERY",
                CLOUD_RUN_WORKER_POOL_DISCOVERY,
            )]),
            "8081",
            &metadata,
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn explicit_worker_endpoint_overrides_cloud_run_metadata_discovery() {
        let metadata = MockMetadataClient::success("10.0.0.15");
        let endpoints = discover_worker_endpoints_with_metadata_client(
            env(&[
                (
                    "TALON_WORKER_ENDPOINT_DISCOVERY",
                    CLOUD_RUN_WORKER_POOL_DISCOVERY,
                ),
                (
                    "TALON_WORKER_ENDPOINT_URL",
                    "http://worker.example.com:8081",
                ),
            ]),
            "9090",
            &metadata,
        )
        .await
        .unwrap();

        assert_eq!(endpoints[0].url, "http://worker.example.com:8081");
        assert!(metadata.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn disabled_cloud_run_discovery_does_not_lookup_metadata() {
        let metadata = MockMetadataClient::success("10.0.0.15");
        let endpoints = discover_worker_endpoints_with_metadata_client(env(&[]), "8081", &metadata)
            .await
            .unwrap();

        assert!(endpoints.is_empty());
        assert!(metadata.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cloud_run_service_url_is_not_used_when_worker_pool_discovery_is_enabled() {
        let metadata = MockMetadataClient::success("10.0.0.15");
        let endpoints = discover_worker_endpoints_with_metadata_client(
            env(&[
                (
                    "TALON_WORKER_ENDPOINT_DISCOVERY",
                    CLOUD_RUN_WORKER_POOL_DISCOVERY,
                ),
                ("CLOUD_RUN_SERVICE_URL", "https://service.example.com"),
            ]),
            "8081",
            &metadata,
        )
        .await
        .unwrap();

        assert_eq!(endpoints[0].url, "http://10.0.0.15:8081");
    }

    #[tokio::test]
    async fn worker_endpoint_discovery_ignores_cloud_run_worker_pool_without_url() {
        let endpoints = discover_worker_endpoints(
            |name| match name {
                "K_CONFIGURATION" => Some("worker-pool-a".to_string()),
                "K_REVISION" => Some("worker-pool-a-00001".to_string()),
                _ => None,
            },
            "8081",
        )
        .await
        .unwrap();

        assert!(endpoints.is_empty());
    }

    #[test]
    fn first_ecs_ipv4_address_reads_container_metadata() {
        let metadata = serde_json::json!({
            "Networks": [
                {
                    "NetworkMode": "awsvpc",
                    "IPv4Addresses": ["10.0.12.34"]
                }
            ]
        });

        assert_eq!(first_ecs_ipv4_address(&metadata), Some("10.0.12.34"));
    }

    #[tokio::test]
    async fn heartbeat_recreates_missing_worker_record() {
        let cp = control_plane();
        let registration = WorkerRegistration::new("worker-a", "1.2.3");
        patch_ready_with_registration_retry(&cp, &registration).await;

        let store = crate::control::resources::ResourceStore::new(cp.kv.clone(), cp.pubsub.clone());
        assert!(store
            .get(ns::TALON_SYSTEM, "Worker", "worker-a")
            .await
            .unwrap()
            .is_some());
    }

    #[test]
    fn worker_liveness_is_based_on_expiry() {
        let mut status = resources_proto::WorkerStatus {
            expires_at: 1_001,
            ..Default::default()
        };
        assert!(worker_is_live(&status, 1_000));
        assert!(!worker_is_stale(&status, 1_000));

        status.expires_at = 1_000;
        assert!(!worker_is_live(&status, 1_000));
        assert!(worker_is_stale(&status, 1_000));
    }
}
