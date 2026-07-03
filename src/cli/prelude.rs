// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

mod auth;
mod commands;

use commands::Cli;

pub(super) use auth::{
    clear_stored_gateway_auth, connect_gateway, describe_stored_auth, exchange_oidc_id_token,
    login_with_google_loopback, mint_local_platform_access_jwt, parse_api_key_grant,
    resolve_token_ttl_seconds, save_stored_gateway_auth, LocalPlatformTokenScope,
    DEFAULT_TOKEN_TTL,
};
