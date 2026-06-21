// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod auth;
pub mod oidc;
pub mod rest;
pub mod rpc;
pub mod server;
pub mod session_streams;
pub mod ui;

pub use server::Gateway;
