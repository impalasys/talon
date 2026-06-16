// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

mod apply;
mod auth;
mod delete;
mod gen;
mod get;
mod knowledge;
mod render;
mod workflow;

pub(crate) use apply::ApplyCommand;
pub(crate) use auth::AuthCommand;
pub(crate) use delete::DeleteCommand;
pub(crate) use gen::GenCommand;
pub(crate) use get::GetCommand;
pub(crate) use knowledge::KnowledgeCommand;
pub(crate) use render::RenderCommand;
pub(crate) use workflow::WorkflowCommand;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "talon-cli")]
#[command(about = "Administration CLI for the Talon system", long_about = None)]
pub(crate) struct Cli {
    /// gRPC gateway address (e.g. http://localhost:50051)
    #[arg(long, default_value = "http://localhost:50051")]
    pub(crate) gateway: String,

    /// Gateway password for Basic auth. Uses username "" and password value.
    #[arg(long)]
    pub(crate) password: Option<String>,

    /// Gateway bearer token.
    #[arg(long)]
    pub(crate) token: Option<String>,

    /// Shared JWT secret for minting a short-lived Talon admin token.
    #[arg(long)]
    pub(crate) jwt_secret: Option<String>,

    /// Use the REST-transcoded public HTTP endpoints instead of native gRPC.
    #[arg(long, default_value_t = false)]
    pub(crate) rest: bool,

    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Mint scoped auth tokens for clients.
    Auth(AuthCommand),
    /// Manage namespace knowledge artifacts directly by path.
    Knowledge(KnowledgeCommand),
    /// Create and inspect workflow runs.
    Workflow(WorkflowCommand),
    /// Applies a manifest file (e.g. Agent, Template, Deployment)
    Apply(ApplyCommand),
    /// Renders a manifest file after template substitution.
    Render(RenderCommand),
    /// Retrieves a manifest from the gateway.
    ///
    /// Supported resource kinds:
    ///   agent, template, mcp-server, knowledge, schedule, channel, channel-subscription
    Get(GetCommand),
    /// Deletes a manifest from the gateway.
    ///
    /// Supported resource kinds:
    ///   template, mcp-server, knowledge, channel, channel-subscription
    Delete(DeleteCommand),
    /// Generates a TypeScript client SDK from manifest files
    Gen(GenCommand),
}

use anyhow::Result;

pub(super) struct RunOutcome {
    pub(super) exit_code: Option<i32>,
}

pub(super) async fn run_cli(cli: &Cli) -> Result<RunOutcome> {
    match &cli.command {
        Commands::Auth(command) => return auth::run(cli, command).await,
        Commands::Knowledge(command) => return knowledge::run(cli, command).await,
        Commands::Workflow(command) => return workflow::run(cli, command).await,
        Commands::Apply(command) => apply::run(cli, command).await?,
        Commands::Render(command) => return render::run(cli, command).await,
        Commands::Get(command) => get::run(cli, command).await?,
        Commands::Delete(command) => return delete::run(cli, command).await,
        Commands::Gen(command) => return gen::run(cli, command).await,
    }

    Ok(RunOutcome { exit_code: None })
}
