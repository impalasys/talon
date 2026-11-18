// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! This consumer's view of its direct internal dependencies.

#![allow(unused_imports)]

pub use ::contracts;
pub use ::protocol;
#[cfg(test)]
pub use ::test_support;
