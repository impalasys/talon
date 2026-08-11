// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{ns, ControlPlane};
use crate::gateway::rpc::resources_proto;
use anyhow::{Context, Result};
use serde_json::Value;
use std::net::Ipv4Addr;
use std::sync::{Arc, OnceLock};
use tokio_util::sync::CancellationToken;
use url::Url;

pub const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
pub const HEARTBEAT_TTL: chrono::Duration = chrono::Duration::seconds(30);

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

pub async fn discover_worker_endpoints<F>(
    get: F,
    port: &str,
) -> Result<Vec<resources_proto::WorkerEndpoint>>
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(endpoint) = explicit_worker_endpoint(&get) {
        return Ok(vec![endpoint]);
    }

    if let Some(endpoint) = ecs_worker_endpoint(&get, port).await {
        return Ok(vec![endpoint]);
    }

    if let Some(endpoint) =
        cloud_run_worker_pool_endpoint(&get, port, CLOUD_RUN_METADATA_URL).await?
    {
        return Ok(vec![endpoint]);
    }

    Ok(Vec::new())
}

fn explicit_worker_endpoint<F>(get: &F) -> Option<resources_proto::WorkerEndpoint>
where
    F: Fn(&str) -> Option<String>,
{
    [
        "TALON_WORKER_ENDPOINT_URL",
        "TALON_WORKER_PUBLIC_URL",
        "TALON_WORKER_URL",
    ]
    .into_iter()
    .find_map(|name| get(name).and_then(|url| worker_endpoint_from_url(&url, get)))
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

async fn cloud_run_worker_pool_endpoint<F>(
    get: &F,
    port: &str,
    metadata_url: &str,
) -> Result<Option<resources_proto::WorkerEndpoint>>
where
    F: Fn(&str) -> Option<String>,
{
    if !get("CLOUD_RUN_WORKER_POOL").is_some_and(|value| !value.trim().is_empty()) {
        return Ok(None);
    }

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(CLOUD_RUN_METADATA_TIMEOUT)
        .build()
        .context("failed to build Cloud Run metadata client")?;
    for attempt in 1..=CLOUD_RUN_METADATA_MAX_ATTEMPTS {
        let result = async {
            let response = client
                .get(metadata_url)
                .header("Metadata-Flavor", "Google")
                .send()
                .await
                .context("metadata request failed")?;
            if !response.status().is_success() {
                anyhow::bail!("metadata server returned HTTP {}", response.status());
            }
            let address = response
                .text()
                .await
                .context("failed to read metadata response")?
                .trim()
                .parse::<Ipv4Addr>()
                .context("metadata response was not a valid IPv4 address")?;
            if !is_usable_cloud_run_ipv4(address) {
                anyhow::bail!("metadata response was not a usable VPC IPv4 address");
            }
            Ok::<Ipv4Addr, anyhow::Error>(address)
        }
        .await;

        match result {
            Ok(address) => {
                return Ok(worker_endpoint_from_url(
                    &format!("http://{}:{}", address, port),
                    get,
                ));
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
                anyhow::bail!(
                    "Cloud Run Worker Pool endpoint discovery produced no usable VPC IPv4 address; verify Direct VPC ingress, metadata access, and gateway VPC reachability"
                );
            }
        }
    }
    Ok(None)
}

fn is_usable_cloud_run_ipv4(address: Ipv4Addr) -> bool {
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_link_local()
        && !address.is_multicast()
        && !address.is_broadcast()
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
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::task::JoinHandle;

    struct MetadataServer {
        url: String,
        requests: Arc<Mutex<Vec<String>>>,
        task: JoinHandle<()>,
    }

    impl Drop for MetadataServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn metadata_server(body: &str) -> MetadataServer {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured_requests = requests.clone();
        let body = body.to_string();
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut request = vec![0; 4096];
                let size = tokio::time::timeout(
                    std::time::Duration::from_secs(1),
                    socket.read(&mut request),
                )
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
                request.truncate(size);
                captured_requests
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&request).into_owned());
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        MetadataServer {
            url: format!("http://{}", address),
            requests,
            task,
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
    async fn cloud_run_worker_pool_discovery_auto_detects_private_ipv4_and_port() {
        let server = metadata_server("10.0.0.15\n").await;
        let environment = env(&[("CLOUD_RUN_WORKER_POOL", "worker-pool-a")]);
        let endpoint = cloud_run_worker_pool_endpoint(&environment, "9090", &server.url)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(endpoint.url, "http://10.0.0.15:9090");
        assert_eq!(endpoint.protocol, "http");
        assert!(server.requests.lock().unwrap()[0]
            .to_ascii_lowercase()
            .contains("metadata-flavor: google"));
    }

    #[tokio::test]
    async fn cloud_run_worker_pool_discovery_accepts_non_rfc1918_vpc_ipv4() {
        let server = metadata_server("100.64.0.15").await;
        let environment = env(&[("CLOUD_RUN_WORKER_POOL", "worker-pool-a")]);
        let endpoint = cloud_run_worker_pool_endpoint(&environment, "8081", &server.url)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(endpoint.url, "http://100.64.0.15:8081");
    }

    #[tokio::test]
    async fn cloud_run_worker_pool_discovery_rejects_invalid_or_empty_metadata() {
        for response in [
            "",
            "not-an-ip",
            "2001:db8::1",
            "0.0.0.0",
            "169.254.1.1",
            "224.0.0.1",
            "255.255.255.255",
        ] {
            let server = metadata_server(response).await;
            let environment = env(&[("CLOUD_RUN_WORKER_POOL", "worker-pool-a")]);
            let result = cloud_run_worker_pool_endpoint(&environment, "8081", &server.url).await;
            assert!(result.is_err(), "unexpected endpoint for {response:?}");
        }
    }

    #[tokio::test]
    async fn cloud_run_worker_pool_discovery_retries_metadata_failure() {
        let environment = env(&[("CLOUD_RUN_WORKER_POOL", "worker-pool-a")]);
        let result =
            cloud_run_worker_pool_endpoint(&environment, "8081", "http://127.0.0.1:1/metadata")
                .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cloud_run_worker_pool_discovery_rejects_localhost() {
        let server = metadata_server("127.0.0.1").await;
        let environment = env(&[("CLOUD_RUN_WORKER_POOL", "worker-pool-a")]);
        let result = cloud_run_worker_pool_endpoint(&environment, "8081", &server.url).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn explicit_worker_endpoint_overrides_cloud_run_metadata_discovery() {
        let endpoints = discover_worker_endpoints(
            env(&[
                ("CLOUD_RUN_WORKER_POOL", "worker-pool-a"),
                (
                    "TALON_WORKER_ENDPOINT_URL",
                    "http://worker.example.com:8081",
                ),
            ]),
            "9090",
        )
        .await
        .unwrap();

        assert_eq!(endpoints[0].url, "http://worker.example.com:8081");
    }

    #[tokio::test]
    async fn explicit_endpoint_candidates_skip_invalid_values() {
        let endpoints = discover_worker_endpoints(
            env(&[
                ("CLOUD_RUN_WORKER_POOL", "worker-pool-a"),
                ("TALON_WORKER_ENDPOINT_URL", "not-a-url"),
                ("TALON_WORKER_PUBLIC_URL", "http://worker.example.com:8081"),
            ]),
            "9090",
        )
        .await
        .unwrap();

        assert_eq!(endpoints[0].url, "http://worker.example.com:8081");
    }

    #[tokio::test]
    async fn disabled_cloud_run_discovery_does_not_lookup_metadata() {
        let endpoints = discover_worker_endpoints(env(&[]), "8081").await.unwrap();

        assert!(endpoints.is_empty());
    }

    #[tokio::test]
    async fn cloud_run_service_url_is_not_used_as_worker_endpoint() {
        let endpoints = discover_worker_endpoints(
            env(&[("CLOUD_RUN_SERVICE_URL", "https://service.example.com")]),
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
