// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use super::{Cli, RunOutcome};
use crate::cli::{
    clear_stored_gateway_auth, describe_stored_auth, exchange_oidc_id_token,
    login_with_google_loopback, mint_agent_jwt, mint_channel_jwt, mint_root_jwt, mint_session_jwt,
    resolve_gateway_jwt_secret, save_stored_gateway_auth,
};

#[derive(Args)]
pub(crate) struct AuthCommand {
    #[command(subcommand)]
    command: AuthCommands,
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Sign in through Google OIDC and store a short-lived Talon access token.
    Login {
        /// Google ID token to exchange directly. If omitted, opens a browser loopback OAuth flow.
        #[arg(long)]
        id_token: Option<String>,
        /// OIDC trust entry name to require during exchange.
        #[arg(long)]
        trust: Option<String>,
        /// Google Desktop OAuth client id. Defaults to TALON_GOOGLE_CLIENT_ID, TALON_GOOGLE_CLI_CLIENT_ID,
        /// or the built-in Talon CLI client id.
        #[arg(long)]
        google_client_id: Option<String>,
        /// Google Desktop OAuth client secret. Defaults to TALON_GOOGLE_CLIENT_SECRET or
        /// TALON_GOOGLE_CLI_CLIENT_SECRET.
        #[arg(long)]
        google_client_secret: Option<String>,
    },
    /// Remove stored Talon CLI auth.
    Logout,
    /// Show stored Talon CLI auth.
    Whoami,
    /// Mint a root JWT with unrestricted gateway scope.
    RootToken {
        #[arg(long, default_value = "talon-root-client")]
        subject: String,
        #[arg(long, default_value_t = 3600)]
        ttl_seconds: u64,
    },
    /// Mint a JWT that can only access one agent in a namespace.
    AgentToken {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        #[arg(long, default_value = "talon-agent-client")]
        subject: String,
        #[arg(long, default_value_t = 3600)]
        ttl_seconds: u64,
    },
    /// Mint a JWT that can only access one session for one agent.
    SessionToken {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        #[arg(short, long)]
        session: String,
        #[arg(long, default_value = "talon-session-client")]
        subject: String,
        #[arg(long, default_value_t = 3600)]
        ttl_seconds: u64,
    },
    /// Mint a JWT that can only access messages in one channel.
    ChannelToken {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        channel: String,
        #[arg(long, default_value = "talon-channel-client")]
        subject: String,
        #[arg(long, default_value_t = 3600)]
        ttl_seconds: u64,
    },
}

pub(super) async fn run(cli: &Cli, command: &AuthCommand) -> Result<RunOutcome> {
    let token = match &command.command {
        AuthCommands::Login {
            id_token,
            trust,
            google_client_id,
            google_client_secret,
        } => {
            let auth = if let Some(id_token) = id_token {
                exchange_oidc_id_token(cli, id_token, trust.as_deref(), "cli").await?
            } else {
                login_with_google_loopback(
                    cli,
                    google_client_id.as_deref(),
                    google_client_secret.as_deref(),
                    trust.as_deref(),
                )
                .await?
            };
            save_stored_gateway_auth(&auth)?;
            println!(
                "Logged in to {} as {} ({})",
                auth.gateway,
                auth.email.as_deref().unwrap_or(&auth.subject),
                auth.trust
            );
            return Ok(RunOutcome { exit_code: None });
        }
        AuthCommands::Logout => {
            if clear_stored_gateway_auth()? {
                println!("Logged out");
            } else {
                println!("No stored auth found");
            }
            return Ok(RunOutcome { exit_code: None });
        }
        AuthCommands::Whoami => {
            match describe_stored_auth(cli)? {
                Some(description) => println!("{description}"),
                None => println!("Not logged in"),
            }
            return Ok(RunOutcome { exit_code: None });
        }
        AuthCommands::RootToken {
            subject,
            ttl_seconds,
        } => {
            let secret = resolve_gateway_jwt_secret(cli)
                .context("TALON_JWT_SECRET or GATEWAY_JWT_SECRET is required")?;
            mint_root_jwt(&secret, subject, *ttl_seconds)?
        }
        AuthCommands::AgentToken {
            namespace,
            agent,
            subject,
            ttl_seconds,
        } => {
            let secret = resolve_gateway_jwt_secret(cli)
                .context("TALON_JWT_SECRET or GATEWAY_JWT_SECRET is required")?;
            mint_agent_jwt(&secret, namespace, agent, subject, *ttl_seconds)?
        }
        AuthCommands::SessionToken {
            namespace,
            agent,
            session,
            subject,
            ttl_seconds,
        } => {
            let secret = resolve_gateway_jwt_secret(cli)
                .context("TALON_JWT_SECRET or GATEWAY_JWT_SECRET is required")?;
            mint_session_jwt(&secret, namespace, agent, session, subject, *ttl_seconds)?
        }
        AuthCommands::ChannelToken {
            namespace,
            channel,
            subject,
            ttl_seconds,
        } => {
            let secret = resolve_gateway_jwt_secret(cli)
                .context("TALON_JWT_SECRET or GATEWAY_JWT_SECRET is required")?;
            mint_channel_jwt(&secret, namespace, channel, subject, *ttl_seconds)?
        }
    };
    println!("{}", token);
    Ok(RunOutcome { exit_code: None })
}
