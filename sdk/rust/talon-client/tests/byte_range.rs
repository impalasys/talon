// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#![allow(deprecated)]

use prost::Message;
use talon_client::harness::{
    chat_content_part, ByteRange, ChatContentPart, ToolOutput, ToolOutputByteRange,
};

fn round_trip<T: Message + Default>(message: &T) -> T {
    let bytes = message.encode_to_vec();
    T::decode(bytes.as_slice()).expect("decode byte-range message")
}

#[test]
fn byte_range_constructor_rejects_inverted_interval() {
    assert!(ByteRange::new(10, 5).is_none());
}

#[test]
fn byte_range_constructor_accepts_ordered_interval() {
    let range = ByteRange::new(3, 8).expect("ordered range");
    assert_eq!(range.start, 3);
    assert_eq!(range.end, 8);
    assert_eq!(range.len(), 5);
    assert!(!range.is_empty());
}

#[test]
fn byte_range_empty_interval_has_zero_len() {
    let range = ByteRange::new(7, 7).expect("empty range");
    assert!(range.is_empty());
    assert_eq!(range.len(), 0);
}

#[test]
fn chat_content_part_byte_range_round_trips() {
    let part = ChatContentPart {
        content: Some(chat_content_part::Content::Text("héllo wörld".to_string())),
        byte_range: ByteRange::new(0, 5),
    };
    let decoded = round_trip(&part);
    assert_eq!(decoded, part);
    assert_eq!(decoded.byte_range.expect("range").end, 5);
}

#[test]
fn tool_output_deprecated_receipt_round_trips() {
    let output = ToolOutput {
        content_parts: vec![ChatContentPart {
            content: Some(chat_content_part::Content::Text("abc".to_string())),
            byte_range: None,
        }],
        summary: "s".to_string(),
        byte_range: ToolOutputByteRange::new(0, 3, Some(3)),
    };
    let decoded = round_trip(&output);
    assert_eq!(decoded, output);
    assert_eq!(decoded.byte_range.expect("receipt").next_byte, Some(3));
}
