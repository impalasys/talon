// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;

use super::{Cli, RunOutcome};
use crate::cli::{grpc_delete_resource, rest_delete_resource};

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
