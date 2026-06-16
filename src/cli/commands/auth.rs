// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use super::{Cli, RunOutcome};
use crate::cli::{
    mint_agent_jwt, mint_channel_jwt, mint_root_jwt, mint_session_jwt, resolve_gateway_jwt_secret,
};

#[derive(Args)]
pub(crate) struct AuthCommand {
    #[command(subcommand)]
    command: AuthCommands,
}

#[derive(Subcommand)]
enum AuthCommands {
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
    let secret = resolve_gateway_jwt_secret(cli)
        .context("TALON_JWT_SECRET or GATEWAY_JWT_SECRET is required")?;
    let token = match &command.command {
        AuthCommands::RootToken {
            subject,
            ttl_seconds,
        } => mint_root_jwt(&secret, subject, *ttl_seconds)?,
        AuthCommands::AgentToken {
            namespace,
            agent,
            subject,
            ttl_seconds,
        } => mint_agent_jwt(&secret, namespace, agent, subject, *ttl_seconds)?,
        AuthCommands::SessionToken {
            namespace,
            agent,
            session,
            subject,
            ttl_seconds,
        } => mint_session_jwt(&secret, namespace, agent, session, subject, *ttl_seconds)?,
        AuthCommands::ChannelToken {
            namespace,
            channel,
            subject,
            ttl_seconds,
        } => mint_channel_jwt(&secret, namespace, channel, subject, *ttl_seconds)?,
    };
    println!("{}", token);
    Ok(RunOutcome { exit_code: None })
}
