// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;

use super::Cli;
use crate::cli::{grpc_get_yaml, rest_get_yaml};

#[derive(clap::Args)]
pub(crate) struct GetCommand {
    /// Type of resource to get: agent, template, mcp-server, knowledge, schedule, channel, channel-subscription
    #[arg(value_name = "KIND")]
    pub(crate) kind: String,
    /// Name of the resource
    ///
    /// Channel subscriptions use '<channel>/<subscription>'.
    #[arg(value_name = "NAME")]
    pub(crate) name: String,
    /// Namespace of the resource
    #[arg(short, long)]
    pub(crate) namespace: Option<String>,
}

pub(super) async fn run(cli: &Cli, command: &GetCommand) -> Result<()> {
    if cli.rest {
        println!(
            "{}",
            rest_get_yaml(
                cli,
                &command.kind,
                &command.name,
                command.namespace.as_ref()
            )
            .await?
        );
        return Ok(());
    }

    println!(
        "{}",
        grpc_get_yaml(
            cli,
            &command.kind,
            &command.name,
            command.namespace.as_ref()
        )
        .await?
    );
    Ok(())
}
