// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

mod apply;
mod auth;
mod delete;
mod file;
mod gen;
mod get;
mod knowledge;
mod render;
mod search;
mod session;
mod workflow;

pub(crate) use apply::ApplyCommand;
pub(crate) use auth::AuthCommand;
pub(crate) use delete::DeleteCommand;
pub(crate) use file::FileCommand;
pub(crate) use gen::GenCommand;
pub(crate) use get::GetCommand;
pub(crate) use knowledge::KnowledgeCommand;
pub(crate) use render::RenderCommand;
pub(crate) use search::SearchCommand;
pub(crate) use session::SessionCommand;
pub(crate) use workflow::WorkflowCommand;

use clap::{Parser, Subcommand};

pub(crate) const DEFAULT_GRPC_GATEWAY: &str = "grpc.talon.impala.systems";
pub(crate) const DEFAULT_GRPC_WEB_GATEWAY: &str = "talon.impala.systems";

#[derive(Parser)]
#[command(name = "talon-cli")]
#[command(about = "Administration CLI for the Talon system", long_about = None)]
pub(crate) struct Cli {
    /// gRPC gateway address (defaults to grpc.talon.impala.systems, or talon.impala.systems with --grpc-web)
    #[arg(long)]
    pub(crate) gateway: Option<String>,

    /// Gateway bearer token.
    #[arg(long)]
    pub(crate) token: Option<String>,

    /// Talon API key to exchange for a short-lived bearer token.
    #[arg(long)]
    pub(crate) api_key: Option<String>,

    /// Grant to request when exchanging a multi-grant API key.
    #[arg(long)]
    pub(crate) api_key_grant: Option<String>,

    /// Use gRPC-Web over HTTP/1.1 instead of native gRPC.
    #[arg(long, default_value_t = false)]
    pub(crate) grpc_web: bool,

    #[command(subcommand)]
    pub(crate) command: Commands,
}

impl Cli {
    pub(crate) fn gateway_url(&self) -> String {
        if let Some(gateway) = self.gateway.as_ref() {
            return gateway.clone();
        }
        if let Ok(env_gateway) = std::env::var("TALON_GATEWAY") {
            let env_gateway = env_gateway.trim();
            if !env_gateway.is_empty() {
                return env_gateway.to_string();
            }
        }
        if self.grpc_web_enabled() {
            DEFAULT_GRPC_WEB_GATEWAY.to_string()
        } else {
            DEFAULT_GRPC_GATEWAY.to_string()
        }
    }

    pub(crate) fn grpc_web_enabled(&self) -> bool {
        self.grpc_web
            || std::env::var("TALON_GRPC_WEB")
                .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
                .unwrap_or(false)
    }
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Mint scoped auth tokens for clients.
    Auth(AuthCommand),
    /// Manage namespace knowledge artifacts directly by path.
    Knowledge(KnowledgeCommand),
    /// Manage namespace Files.
    File(FileCommand),
    /// Search indexed Talon resources.
    Search(SearchCommand),
    /// Create sessions, send prompts, and inspect messages.
    Session(SessionCommand),
    /// Create and inspect workflow runs.
    Workflow(WorkflowCommand),
    /// Applies a manifest file (e.g. Agent, Template, Deployment)
    Apply(ApplyCommand),
    /// Renders a manifest file after template substitution.
    Render(RenderCommand),
    /// Retrieves a manifest from the gateway.
    ///
    /// Supported resource kinds:
    ///   agent, template, mcp-server, knowledge, file, task, schedule, channel, channel-subscription
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
        Commands::File(command) => return file::run(cli, command).await,
        Commands::Search(command) => return search::run(cli, command).await,
        Commands::Session(command) => return session::run(cli, command).await,
        Commands::Workflow(command) => return workflow::run(cli, command).await,
        Commands::Apply(command) => apply::run(cli, command).await?,
        Commands::Render(command) => return render::run(cli, command).await,
        Commands::Get(command) => get::run(cli, command).await?,
        Commands::Delete(command) => return delete::run(cli, command).await,
        Commands::Gen(command) => return gen::run(cli, command).await,
    }

    Ok(RunOutcome { exit_code: None })
}

#[cfg(test)]
mod tests {
    use super::{Cli, DEFAULT_GRPC_GATEWAY, DEFAULT_GRPC_WEB_GATEWAY};
    use clap::Parser;

    fn parse_cli(args: &[&str]) -> Cli {
        Cli::parse_from(std::iter::once("talon-cli").chain(args.iter().copied()))
    }

    fn clear_gateway_env() {
        std::env::remove_var("TALON_GATEWAY");
        std::env::remove_var("TALON_GRPC_WEB");
    }

    #[test]
    fn default_gateway_uses_native_grpc_host() {
        let _guard = crate::test_support::env_lock();
        clear_gateway_env();

        let cli = parse_cli(&["auth", "whoami"]);

        assert_eq!(cli.gateway_url(), DEFAULT_GRPC_GATEWAY);
    }

    #[test]
    fn grpc_web_default_gateway_uses_web_host() {
        let _guard = crate::test_support::env_lock();
        clear_gateway_env();

        let cli = parse_cli(&["--grpc-web", "auth", "whoami"]);

        assert_eq!(cli.gateway_url(), DEFAULT_GRPC_WEB_GATEWAY);
    }

    #[test]
    fn explicit_gateway_overrides_grpc_web_default() {
        let _guard = crate::test_support::env_lock();
        clear_gateway_env();

        let cli = parse_cli(&[
            "--grpc-web",
            "--gateway",
            "http://localhost:50051",
            "auth",
            "whoami",
        ]);

        assert_eq!(cli.gateway_url(), "http://localhost:50051");
    }

    #[test]
    fn talon_gateway_env_overrides_default_gateway() {
        let _guard = crate::test_support::env_lock();
        clear_gateway_env();
        std::env::set_var("TALON_GATEWAY", "http://env-gateway:50051");

        let cli = parse_cli(&["--grpc-web", "auth", "whoami"]);

        assert_eq!(cli.gateway_url(), "http://env-gateway:50051");
        clear_gateway_env();
    }

    #[test]
    fn blank_talon_gateway_env_falls_back_to_default_gateway() {
        let _guard = crate::test_support::env_lock();
        clear_gateway_env();
        std::env::set_var("TALON_GATEWAY", " ");

        let cli = parse_cli(&["auth", "whoami"]);

        assert_eq!(cli.gateway_url(), DEFAULT_GRPC_GATEWAY);
        clear_gateway_env();
    }
}
