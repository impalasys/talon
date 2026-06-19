// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};

use super::{Cli, RunOutcome};
use crate::cli::{auth_interceptor, resource_lookup_target, rest_request_json};
use crate::gateway::rpc::proto::gateway_service_client::GatewayServiceClient;
use crate::gateway::rpc::proto::DeleteResourceRequest;

#[derive(clap::Args)]
pub(crate) struct DeleteCommand {
    /// Type of resource to delete: template, mcp-server, knowledge, channel, channel-subscription
    #[arg(value_name = "KIND")]
    kind: String,
    /// Name of the resource
    ///
    /// Channel subscriptions use '<channel>/<subscription>'.
    #[arg(value_name = "NAME")]
    name: String,
    /// Namespace of the resource
    #[arg(short, long)]
    namespace: Option<String>,
}

pub(super) async fn run(cli: &Cli, command: &DeleteCommand) -> Result<RunOutcome> {
    if cli.rest {
        println!(
            "{}",
            rest_delete_resource(
                cli,
                &command.kind,
                &command.name,
                command.namespace.as_ref()
            )
            .await?
        );
        return Ok(RunOutcome { exit_code: None });
    }
    println!(
        "{}",
        grpc_delete_resource(
            cli,
            &command.kind,
            &command.name,
            command.namespace.as_ref()
        )
        .await?
    );
    Ok(RunOutcome { exit_code: None })
}

async fn grpc_delete_resource(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let channel = tonic::transport::Channel::from_shared(cli.gateway.clone())
        .with_context(|| format!("Invalid gateway URL {}", cli.gateway))?
        .connect()
        .await
        .with_context(|| format!("Could not connect to gateway at {}", cli.gateway))?;
    let mut client = GatewayServiceClient::with_interceptor(channel, auth_interceptor(cli)?);

    let (ns, kind, name) = resource_lookup_target(kind, name, namespace)?;
    client
        .delete_resource(DeleteResourceRequest {
            ns: ns.clone(),
            kind: kind.clone(),
            name: name.clone(),
        })
        .await
        .with_context(|| format!("Failed to delete {} '{}/{}'", kind, ns, name))?;
    Ok(format!(
        "✓ {} '{}/{}' deleted successfully.",
        kind, ns, name
    ))
}

async fn rest_delete_resource(
    cli: &Cli,
    kind: &str,
    name: &str,
    namespace: Option<&String>,
) -> Result<String> {
    let path = rest_delete_path(kind, name, namespace)?;
    rest_request_json(cli, reqwest::Method::DELETE, &path, None)
        .await
        .with_context(|| format!("Failed to delete {} '{}'", kind, name))?;
    Ok(format!("✓ {} '{}' deleted successfully.", kind, name))
}

fn rest_delete_path(kind: &str, name: &str, namespace: Option<&String>) -> Result<String> {
    if matches!(kind.to_lowercase().as_str(), "namespace" | "namespaces") {
        return Ok(format!("/v1/namespaces/{}", urlencoding::encode(name)));
    }
    let (ns, resource_kind, resource_name) = resource_lookup_target(kind, name, namespace)?;
    Ok(format!(
        "/v1/ns/{}/resources/{}/{}",
        urlencoding::encode(&ns),
        urlencoding::encode(&resource_kind),
        urlencoding::encode(&resource_name)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_delete_path_honors_explicit_namespace() {
        let namespace = "customers:source".to_string();

        assert_eq!(
            rest_delete_path("template", "coding-sandbox-policy", Some(&namespace)).unwrap(),
            "/v1/ns/customers%3Asource/resources/Template/coding-sandbox-policy"
        );
    }

    #[test]
    fn sandbox_class_delete_path_honors_explicit_namespace() {
        let namespace = "Example".to_string();

        assert_eq!(
            rest_delete_path("sandboxclass", "docker-codex", Some(&namespace)).unwrap(),
            "/v1/ns/Example/resources/SandboxClass/docker-codex"
        );
    }
}
