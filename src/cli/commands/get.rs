// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;

use super::Cli;
use crate::cli::{
    grpc_get_json, grpc_get_yaml, grpc_list_resources_json, grpc_list_resources_table,
    rest_get_json, rest_get_yaml, rest_list_resources_json, rest_list_resources_table,
};

#[derive(clap::Args)]
pub(crate) struct GetCommand {
    /// Type of resource to get: agent, template, mcp-server, knowledge, schedule, channel, channel-subscription
    #[arg(value_name = "KIND")]
    pub(crate) kind: String,
    /// Name of the resource. Omit to list resources of this kind.
    ///
    /// Channel subscriptions use '<channel>/<subscription>'.
    #[arg(value_name = "NAME")]
    pub(crate) name: Option<String>,
    /// Namespace of the resource
    #[arg(short, long)]
    pub(crate) namespace: Option<String>,
    /// Output format. Defaults to table for lists and yaml for single resources.
    #[arg(short, long, value_enum)]
    pub(crate) output: Option<GetOutput>,
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum GetOutput {
    Table,
    Yaml,
    Json,
}

pub(super) async fn run(cli: &Cli, command: &GetCommand) -> Result<()> {
    let Some(name) = command.name.as_ref() else {
        let output = match command.output.unwrap_or(GetOutput::Table) {
            GetOutput::Table => {
                if cli.rest {
                    rest_list_resources_table(cli, &command.kind, command.namespace.as_ref())
                        .await?
                } else {
                    grpc_list_resources_table(cli, &command.kind, command.namespace.as_ref())
                        .await?
                }
            }
            GetOutput::Json => {
                let value = if cli.rest {
                    rest_list_resources_json(cli, &command.kind, command.namespace.as_ref()).await?
                } else {
                    grpc_list_resources_json(cli, &command.kind, command.namespace.as_ref()).await?
                };
                serde_json::to_string_pretty(&value)?
            }
            GetOutput::Yaml => anyhow::bail!("list output format 'yaml' is not supported"),
        };
        println!("{}", output);
        return Ok(());
    };

    let output = match command.output.unwrap_or(GetOutput::Yaml) {
        GetOutput::Yaml => {
            if cli.rest {
                rest_get_yaml(cli, &command.kind, name, command.namespace.as_ref()).await?
            } else {
                grpc_get_yaml(cli, &command.kind, name, command.namespace.as_ref()).await?
            }
        }
        GetOutput::Json => {
            let value = if cli.rest {
                rest_get_json(cli, &command.kind, name, command.namespace.as_ref()).await?
            } else {
                grpc_get_json(cli, &command.kind, name, command.namespace.as_ref()).await?
            };
            serde_json::to_string_pretty(&value)?
        }
        GetOutput::Table => anyhow::bail!("single resource output format 'table' is not supported"),
    };
    println!("{}", output);
    Ok(())
}
