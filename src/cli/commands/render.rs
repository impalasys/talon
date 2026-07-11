// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use serde_json::json;

use super::{Cli, RunOutcome};
use crate::cli::{parse_raw_manifest, render_manifest_file};
use crate::control::resource_model::{ChannelSubscriptionResourceExt, TypedResource};

#[derive(clap::Args)]
pub(crate) struct RenderCommand {
    #[arg(short, long)]
    file: String,
    /// Template variables in KEY=VALUE form.
    #[arg(long = "var", value_name = "KEY=VALUE")]
    vars: Vec<String>,
    #[arg(long, default_value = "yaml")]
    format: RenderFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_rejects_worker_manifests() {
        let err = render_json_payload(
            "apiVersion: talon.impalasys.com/v1\nkind: Worker\nmetadata:\n  name: worker-a\n",
        )
        .expect_err("Worker manifests should not be rendered as user-authored resources");

        assert!(err.to_string().contains("Unsupported manifest kind"));
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum RenderFormat {
    Yaml,
    Json,
}

pub(super) async fn run(_cli: &Cli, command: &RenderCommand) -> Result<RunOutcome> {
    let content = render_manifest_file(&command.file, &command.vars)?;
    match command.format {
        RenderFormat::Yaml => {
            print!("{}", content);
        }
        RenderFormat::Json => {
            let payload = render_json_payload(&content)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&payload)
                    .context("Failed to serialize manifest JSON")?
            );
        }
    }
    Ok(RunOutcome { exit_code: None })
}

fn render_json_payload(content: &str) -> Result<serde_json::Value> {
    let raw = parse_raw_manifest(content)?;
    let manifest_value: serde_yaml::Value =
        serde_yaml::from_str(content).context("Failed to parse rendered manifest")?;
    match raw.kind.as_str() {
        "MCPServer" | "McpServer" => Ok(json!({ "server": manifest_value })),
        "Agent" => Ok(json!({ "agent": manifest_value })),
        "Namespace" => {
            let namespace = crate::control::manifest::parse_namespace(content)?;
            Ok(json!({
                "name": namespace.name(),
                "recursive": true,
                "labels": namespace.labels(),
            }))
        }
        "Knowledge" => Ok(json!({ "knowledge": manifest_value })),
        "Channel" => {
            let channel = crate::control::manifest::parse_channel(content)?;
            Ok(json!({ "ns": channel.namespace(), "channel": channel }))
        }
        "ChannelSubscription" => {
            let subscription = crate::control::manifest::parse_channel_subscription(content)?;
            Ok(json!({
                "ns": subscription.namespace(),
                "channel": subscription.channel(),
                "subscription": subscription,
            }))
        }
        "Workflow" => {
            let workflow = crate::control::manifest::parse_workflow(content)?;
            Ok(json!({ "ns": workflow.namespace(), "workflow": workflow }))
        }
        "Template" | "Deployment" | "DeploymentReplica" | "Schedule" | "SandboxClass"
        | "SandboxPolicy" | "Sandbox" | "UsagePolicy" | "ConnectorClass" | "Connector"
        | "Skill" | "File" | "Task" => Ok(json!({ "resource": manifest_value })),
        other => anyhow::bail!("Unsupported manifest kind '{}'", other),
    }
}
