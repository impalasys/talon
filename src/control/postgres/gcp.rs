// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! GCP metadata-server credential adapter for Cloud SQL IAM authentication.

use super::{CredentialProvider, DatabaseCredential};
use anyhow::{anyhow, Context, Result};
use std::time::Duration;

pub struct GcpMetadataCredentialProvider {
    client: reqwest::Client,
}

impl GcpMetadataCredentialProvider {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .context("failed to configure Cloud SQL metadata client")?;
        Ok(Self { client })
    }
}

#[async_trait::async_trait]
impl CredentialProvider for GcpMetadataCredentialProvider {
    async fn credential(&self) -> Result<DatabaseCredential> {
        #[derive(serde::Deserialize)]
        struct MetadataToken {
            access_token: String,
            expires_in: u64,
        }

        let token: MetadataToken = self
            .client
            .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| anyhow!("failed to obtain Cloud SQL IAM access token"))?
            .json()
            .await
            .map_err(|_| anyhow!("invalid Cloud SQL IAM metadata response"))?;
        Ok(DatabaseCredential {
            value: token.access_token,
            valid_for: Duration::from_secs(token.expires_in),
        })
    }
}
