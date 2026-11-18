// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! This consumer's view of its direct internal dependencies.

#![allow(unused_imports)]

pub use ::config;
pub use ::contracts;
pub use ::control;
pub use ::identifiers;
pub use ::protocol;
#[cfg(test)]
pub use ::test_support;

pub mod storage {
    pub use ::storage_cloud_tasks as cloud_tasks;
    pub use ::storage_common as common;
    #[cfg(any(feature = "aws"))]
    pub use ::storage_dynamodb as dynamodb;
    #[cfg(any(feature = "aws"))]
    pub use ::storage_eventbridge as eventbridge;
    pub use ::storage_memory as memory;
    pub use ::storage_objects as objects;
    pub use ::storage_postgres as postgres;
    pub use ::storage_pubsub as pubsub;
    #[cfg(any(feature = "rocksdb"))]
    pub use ::storage_rocksdb as rocksdb;
    pub use ::storage_sqlite as sqlite;
    #[cfg(any(feature = "aws"))]
    pub use ::storage_sqs as sqs;
}
