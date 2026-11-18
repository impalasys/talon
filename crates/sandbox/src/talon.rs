// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! This consumer's view of its direct internal dependencies.

#![allow(unused_imports)]

pub use ::config;
pub use ::contracts;
pub use ::control;
pub use ::identifiers;
pub use ::protocol;
pub use ::security;
#[cfg(test)]
pub use ::test_support;
