// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use bollard::Docker;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub image: String,
    pub cpu_limit: f64,
    pub mem_limit: i64,
}

#[async_trait::async_trait]
pub trait SandboxProvider: Send + Sync {
    async fn create_sandbox(&self, config: SandboxConfig) -> Result<String>;
    async fn destroy_sandbox(&self, id: &str) -> Result<()>;
}

pub struct DockerSandbox {
    pub docker: Docker,
}

impl DockerSandbox {
    pub fn new() -> Self {
        Self {
            docker: Docker::connect_with_local_defaults().unwrap(),
        }
    }
}

#[async_trait::async_trait]
impl SandboxProvider for DockerSandbox {
    async fn create_sandbox(&self, config: SandboxConfig) -> Result<String> {
        println!(
            "DockerSandbox: Creating container from image {}",
            config.image
        );
        Ok("container-id-123".to_string())
    }

    async fn destroy_sandbox(&self, id: &str) -> Result<()> {
        println!("DockerSandbox: Destroying container {}", id);
        Ok(())
    }
}
