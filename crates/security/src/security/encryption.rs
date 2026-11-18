// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::{EncryptedResult, TaskResult};
use anyhow::Result;

pub trait SecurityProvider: Send + Sync {
    fn encrypt(&self, result: TaskResult) -> Result<EncryptedResult>;
    fn decrypt(&self, result: EncryptedResult) -> Result<TaskResult>;
    fn authorize(&self, owner: &str, task_id: &str) -> bool;
}

pub struct BasicSecurityProvider;

impl SecurityProvider for BasicSecurityProvider {
    fn encrypt(&self, result: TaskResult) -> Result<EncryptedResult> {
        Ok(EncryptedResult {
            task_id: result.task_id,
            data: result.output.into_bytes(),
            nonce: vec![0; 12],
        })
    }

    fn decrypt(&self, result: EncryptedResult) -> Result<TaskResult> {
        Ok(TaskResult {
            task_id: result.task_id,
            output: String::from_utf8(result.data)?,
        })
    }

    fn authorize(&self, _owner: &str, _task_id: &str) -> bool {
        true
    }
}

pub struct AesSecurityProvider {
    pub key: [u8; 32],
}

impl SecurityProvider for AesSecurityProvider {
    fn encrypt(&self, result: TaskResult) -> Result<EncryptedResult> {
        // Implement real AES-GCM encryption here
        Ok(EncryptedResult {
            task_id: result.task_id,
            data: result.output.into_bytes(),
            nonce: vec![0; 12],
        })
    }

    fn decrypt(&self, result: EncryptedResult) -> Result<TaskResult> {
        Ok(TaskResult {
            task_id: result.task_id,
            output: String::from_utf8(result.data)?,
        })
    }

    fn authorize(&self, _owner: &str, _task_id: &str) -> bool {
        true
    }
}
