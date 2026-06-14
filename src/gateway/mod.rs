// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod a2a;
pub mod auth;
pub mod rpc;
pub mod server;
pub mod session_streams;
pub mod ui;

pub use server::Gateway;
