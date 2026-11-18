// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! This consumer's view of its direct internal dependencies.

#![allow(unused_imports)]

pub use ::auth;
pub use ::bootstrap;
pub use ::cli;
pub use ::config;
pub use ::connectors;
pub use ::contracts;
pub use ::control;
pub use ::fanout;
pub use ::gateway;
pub use ::harness;
pub use ::identifiers;
pub use ::monty;
pub use ::observability;
pub use ::protocol;
pub use ::sandbox;
pub use ::security;
pub use ::session_control;
pub use ::test_support;
pub use ::worker;
pub use ::workflows;

pub mod storage {
    pub use ::storage_cloud_tasks as cloud_tasks;
    pub use ::storage_common as common;
    #[cfg(feature = "aws")]
    pub use ::storage_dynamodb as dynamodb;
    #[cfg(feature = "aws")]
    pub use ::storage_eventbridge as eventbridge;
    pub use ::storage_memory as memory;
    pub use ::storage_objects as objects;
    pub use ::storage_postgres as postgres;
    pub use ::storage_pubsub as pubsub;
    #[cfg(feature = "rocksdb")]
    pub use ::storage_rocksdb as rocksdb;
    pub use ::storage_sqlite as sqlite;
    #[cfg(feature = "aws")]
    pub use ::storage_sqs as sqs;
}
