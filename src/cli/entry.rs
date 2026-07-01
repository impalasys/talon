// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub async fn main() -> Result<()> {
    crate::control::security::install_jwt_crypto_provider();
    crate::control::security::install_rustls_crypto_provider();
    let cli = Cli::parse();

    let outcome = commands::run_cli(&cli).await?;
    if let Some(code) = outcome.exit_code {
        std::process::exit(code);
    }

    Ok(())
}
