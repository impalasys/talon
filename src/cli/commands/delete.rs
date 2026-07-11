// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};

use super::{Cli, RunOutcome};
use crate::cli::{connect_gateway, resource_lookup_target};
use talon_client::v1::DeleteResourceRequest;

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
    let mut client = connect_gateway(cli).await?;

    let (ns, kind, name) =
        resource_lookup_target(&command.kind, &command.name, command.namespace.as_ref())?;
    client
        .delete_resource(DeleteResourceRequest {
            ns: ns.clone(),
            kind: kind.clone(),
            name: name.clone(),
        })
        .await
        .with_context(|| format!("Failed to delete {} '{}/{}'", kind, ns, name))?;

    println!("✓ {} '{}/{}' deleted successfully.", kind, ns, name);
    Ok(RunOutcome { exit_code: None })
}
