// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::gateway::rpc::{resources_proto, worker_proto};
use hyper_util::rt::TokioIo;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use url::Url;

#[derive(Default)]
pub(crate) struct WorkerConnectionPool {
    channels: Mutex<HashMap<String, Channel>>,
}

impl WorkerConnectionPool {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) async fn fanout_client(
        &self,
        endpoint: &resources_proto::WorkerEndpoint,
    ) -> std::result::Result<
        worker_proto::fanout_service_client::FanoutServiceClient<Channel>,
        tonic::Status,
    > {
        Ok(
            worker_proto::fanout_service_client::FanoutServiceClient::new(
                self.channel(endpoint).await?,
            ),
        )
    }

    async fn channel(
        &self,
        endpoint: &resources_proto::WorkerEndpoint,
    ) -> std::result::Result<Channel, tonic::Status> {
        let url = endpoint.url.clone();
        if let Some(channel) = self.channels.lock().await.get(&url).cloned() {
            return Ok(channel);
        }

        let parsed = Url::parse(&url)
            .map_err(|err| tonic::Status::unavailable(format!("invalid worker endpoint: {err}")))?;
        let channel = if parsed.scheme() == "unix" {
            if parsed.path().is_empty() {
                return Err(tonic::Status::unavailable(
                    "unix worker endpoint is missing a socket path",
                ));
            }
            let path = urlencoding::decode(parsed.path()).map_err(|err| {
                tonic::Status::unavailable(format!("invalid unix worker endpoint path: {err}"))
            })?;
            let path = Arc::new(PathBuf::from(path.into_owned()));
            Endpoint::try_from("http://[::]:50051")
                .map_err(|err| {
                    tonic::Status::unavailable(format!("invalid worker endpoint: {err}"))
                })?
                .connect_with_connector(service_fn(move |_uri: Uri| {
                    let path = path.clone();
                    async move { UnixStream::connect(path.as_ref()).await.map(TokioIo::new) }
                }))
                .await
                .map_err(|err| {
                    tonic::Status::unavailable(format!("failed to connect to worker: {err}"))
                })?
        } else {
            Channel::from_shared(url.clone())
                .map_err(|err| {
                    tonic::Status::unavailable(format!("invalid worker endpoint: {err}"))
                })?
                .connect()
                .await
                .map_err(|err| {
                    tonic::Status::unavailable(format!("failed to connect to worker: {err}"))
                })?
        };
        let channel = self
            .channels
            .lock()
            .await
            .entry(url)
            .or_insert_with(|| channel.clone())
            .clone();
        Ok(channel)
    }

    #[cfg(test)]
    pub(crate) async fn cached_channel_count(&self) -> usize {
        self.channels.lock().await.len()
    }
}
