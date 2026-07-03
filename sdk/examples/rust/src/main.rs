// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use talon_client::{
    v1::{CreateNamespaceRequest, ListNamespacesRequest},
    NativeTalonClient,
};
use talon_server::{Options, Server};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = Server::start(Options::default())?;
    let mut client =
        NativeTalonClient::connect(format!("http://{}", server.grpc_endpoint())).await?;

    client
        .namespaces
        .create(CreateNamespaceRequest {
            name: "example-app".to_string(),
            ..Default::default()
        })
        .await?;

    let response = client
        .namespaces
        .list(ListNamespacesRequest::default())
        .await?
        .into_inner();
    println!(
        "Talon is running at {} with {} namespace(s)",
        server.grpc_endpoint(),
        response.namespaces.len()
    );

    server.stop()?;
    Ok(())
}
