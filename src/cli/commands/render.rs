// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};

use super::{Cli, RunOutcome};
use crate::cli::{render_json_payload, render_manifest_file};

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
