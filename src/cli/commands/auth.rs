// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;
use talon_client::data::ApiKeyGrant;
use talon_client::v1::{CreateApiKeyRequest, ListApiKeysRequest, RevokeApiKeyRequest};

use super::{Cli, RunOutcome};
use crate::cli::{
    clear_stored_gateway_auth, describe_stored_auth, exchange_oidc_id_token,
    login_with_google_loopback, mint_local_platform_access_jwt, parse_api_key_grant,
    resolve_token_ttl_seconds, save_stored_gateway_auth, LocalPlatformTokenScope,
    DEFAULT_TOKEN_TTL,
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
    /// Mint a local-only RS256 Talon access token from a platform private PEM.
    LocalToken {
        /// File containing the RSA private key PEM. Intended for local bootstrap only.
        #[arg(long)]
        private_key_pem_file: PathBuf,
        #[arg(long, default_value = "talon-root-client")]
        subject: String,
        /// Token lifetime, such as 5min, 1wk, 3mo, or 1yr.
        #[arg(long, default_value = DEFAULT_TOKEN_TTL)]
        ttl: String,
        /// Token lifetime in seconds. Kept for script compatibility.
        #[arg(long)]
        ttl_seconds: Option<u64>,
        /// Optional namespace scope. Omit all scope and grant options to mint a root bootstrap token.
        #[arg(short, long)]
        namespace: Option<String>,
        /// Optional agent scope.
        #[arg(short, long)]
        agent: Option<String>,
        /// Optional session scope.
        #[arg(short, long)]
        session: Option<String>,
        /// Optional channel scope.
        #[arg(short, long)]
        channel: Option<String>,
        /// Optional grant syntax: read|readwrite[,namespace=ns][,agent=name][,session=id][,channel=name].
        #[arg(long = "grant")]
        grants: Vec<String>,
        /// Browser origin allowed to use this token. Repeat for multiple origins.
        #[arg(long = "origin")]
        origins: Vec<String>,
    },
    /// Manage revocable API keys.
    ApiKey {
        #[command(subcommand)]
        command: ApiKeyCommands,
    },
}

#[derive(Subcommand)]
enum ApiKeyCommands {
    /// Create an API key. The secret is printed once.
    Create {
        #[arg(long)]
        name: String,
        /// Grant syntax: read|readwrite[,namespace=ns][,agent=name][,session=id][,channel=name].
        #[arg(long = "grant", required = true)]
        grants: Vec<String>,
        /// Absolute Unix expiry timestamp for the API key.
        #[arg(long)]
        expires_at: Option<u64>,
    },
    /// List API keys without secret material.
    List,
    /// Revoke an API key by id.
    Revoke { id: String },
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
        AuthCommands::LocalToken {
            private_key_pem_file,
            namespace,
            agent,
            session,
            channel,
            subject,
            ttl,
            ttl_seconds,
            grants,
            origins,
        } => {
            let ttl_seconds = resolve_token_ttl_seconds(ttl, *ttl_seconds)?;
            let private_key_pem = std::fs::read_to_string(private_key_pem_file).with_context(
                || {
                    format!(
                        "Failed to read private key PEM file {}",
                        private_key_pem_file.display()
                    )
                },
            )?;
            let parsed_grants = grants
                .iter()
                .map(|grant| parse_api_key_grant(grant))
                .collect::<Result<Vec<_>>>()?;
            mint_local_platform_access_jwt(
                &private_key_pem,
                subject,
                ttl_seconds,
                LocalPlatformTokenScope {
                    namespace: namespace.as_deref(),
                    agent: agent.as_deref(),
                    session: session.as_deref(),
                    channel: channel.as_deref(),
                },
                origins,
                &parsed_grants,
            )?
        }
        AuthCommands::ApiKey { command } => {
            run_api_key_command(cli, command).await?;
            return Ok(RunOutcome { exit_code: None });
        }
    };
    println!("{}", token);
    Ok(RunOutcome { exit_code: None })
}

async fn run_api_key_command(cli: &Cli, command: &ApiKeyCommands) -> Result<()> {
    let mut client = crate::cli::connect_gateway(cli).await?;
    match command {
        ApiKeyCommands::Create {
            name,
            grants,
            expires_at,
        } => {
            let response = client
                .create_api_key(CreateApiKeyRequest {
                    name: name.clone(),
                    grants: grants
                        .iter()
                        .map(|grant| parse_api_key_grant(grant))
                        .collect::<Result<Vec<_>>>()?,
                    expires_at: *expires_at,
                })
                .await
                .context("Failed to create API key")?
                .into_inner();
            let api_key = response
                .api_key
                .context("API key response missing metadata")?;
            println!("id={}", api_key.id);
            println!("prefix={}", api_key.prefix);
            println!("secret={}", response.secret);
        }
        ApiKeyCommands::List => {
            let response = client
                .list_api_keys(ListApiKeysRequest {})
                .await
                .context("Failed to list API keys")?
                .into_inner();
            for key in response.api_keys {
                println!(
                    "id={} name={} prefix={} grants={} created_at={} last_used_at={} expires_at={} revoked_at={}",
                    key.id,
                    key.name,
                    key.prefix,
                    key.grants
                        .iter()
                        .map(format_grant)
                        .collect::<Vec<_>>()
                        .join(";"),
                    key.created_at,
                    key.last_used_at,
                    key.expires_at
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    key.revoked_at
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "-".to_string())
                );
            }
        }
        ApiKeyCommands::Revoke { id } => {
            let response = client
                .revoke_api_key(RevokeApiKeyRequest { id: id.clone() })
                .await
                .context("Failed to revoke API key")?
                .into_inner();
            let api_key = response
                .api_key
                .context("API key response missing metadata")?;
            println!("revoked id={}", api_key.id);
        }
    }
    Ok(())
}

fn format_grant(grant: &ApiKeyGrant) -> String {
    let mut parts = vec![grant.kind.clone()];
    if let Some(namespace) = &grant.namespace {
        parts.push(format!("namespace={namespace}"));
    }
    if let Some(agent) = &grant.agent {
        parts.push(format!("agent={agent}"));
    }
    if let Some(session) = &grant.session {
        parts.push(format!("session={session}"));
    }
    if let Some(channel) = &grant.channel {
        parts.push(format!("channel={channel}"));
    }
    parts.join(",")
}
