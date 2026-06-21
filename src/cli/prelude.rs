// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use clap::Parser;
use minijinja::{context, Environment, UndefinedBehavior};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

mod auth;
mod commands;

use commands::Cli;

pub(super) use auth::{
    connect_gateway, mint_agent_jwt, mint_channel_jwt, mint_root_jwt, mint_session_jwt,
    resolve_gateway_jwt_secret, resolve_gateway_password,
};
