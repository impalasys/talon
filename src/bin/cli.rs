// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[allow(unused_imports)]
mod talon {

    pub use ::cli;
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    talon::cli::main().await
}
