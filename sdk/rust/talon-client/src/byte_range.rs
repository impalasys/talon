// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Minimal constructors for the tool-result byte-range wire contract.
//!
//! These helpers only enforce interval ordering (`end >= start`). UTF-8
//! character-boundary validation and text-eligibility checks belong to the
//! materialization layer, not the wire contract.

use crate::generated::talon::harness::{ByteRange, ToolOutputByteRange};

impl ByteRange {
    /// Half-open byte interval `[start, end)` within a tool-result part's
    /// UTF-8 text. Returns `None` when `end < start`. An empty interval
    /// (`start == end`) is valid.
    pub fn new(start: u64, end: u64) -> Option<Self> {
        if end < start {
            return None;
        }
        Some(ByteRange { start, end })
    }

    /// Length of the interval in bytes.
    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// True when the interval selects no bytes.
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

impl ToolOutputByteRange {
    /// Deprecated page receipt recording the requested byte range for a
    /// read. Returns `None` when `end < start`.
    #[deprecated(note = "page receipt only; must never drive materialization")]
    pub fn new(start: u64, end: u64, next_byte: Option<u64>) -> Option<Self> {
        if end < start {
            return None;
        }
        Some(ToolOutputByteRange {
            start,
            end,
            next_byte,
        })
    }
}
