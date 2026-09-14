// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::BTreeMap;
use std::fmt;

/// A connector feature advertised by a provider adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ConnectorCapability {
    Ingest,
    Deliver,
    Activity,
}

/// Public, non-secret description of a connector implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectorDescriptor {
    pub platform: String,
    pub capabilities: Vec<ConnectorCapability>,
}

/// Framework-neutral representation of a provider request.
///
/// The body intentionally has no `Debug` implementation so it cannot be
/// accidentally emitted by a connector's diagnostics.
#[derive(Clone, Eq, PartialEq)]
pub struct RawInboundRequest {
    headers: BTreeMap<String, Vec<String>>,
    body: Vec<u8>,
}

impl RawInboundRequest {
    /// Preserves duplicate header values and folds field names to ASCII lower
    /// case, as required by HTTP field-name comparison.
    pub fn new(headers: Vec<(String, String)>, body: Vec<u8>) -> Self {
        let mut normalized = BTreeMap::<String, Vec<String>>::new();
        for (name, value) in headers {
            normalized
                .entry(name.to_ascii_lowercase())
                .or_default()
                .push(value);
        }
        Self {
            headers: normalized,
            body,
        }
    }

    pub fn header_values(&self, name: &str) -> Option<&[String]> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(Vec::as_slice)
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

impl fmt::Debug for RawInboundRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawInboundRequest")
            .field("header_count", &self.headers.len())
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// A provider request accepted by a connector's verification stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedInboundRequest {
    pub event_id: String,
    raw: RawInboundRequest,
}

impl VerifiedInboundRequest {
    pub fn new(event_id: String, raw: RawInboundRequest) -> Self {
        Self { event_id, raw }
    }

    pub fn raw(&self) -> &RawInboundRequest {
        &self.raw
    }
}

/// Non-secret Talon routing context selected by a connector host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectorBinding {
    pub registration_id: String,
    pub connector_class: String,
    pub match_fields: BTreeMap<String, String>,
}

/// A provider-independent sender projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sender {
    pub id: String,
    pub display_name: Option<String>,
}

/// An opaque reference returned by an attachment store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentRef {
    pub id: String,
    pub content_type: Option<String>,
}

/// A normalized inbound provider message before host-owned routing metadata is
/// attached.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedInboundMessage {
    pub external_message_id: String,
    pub external_conversation_id: String,
    pub external_thread_id: Option<String>,
    pub conversation_type: String,
    pub sender: Sender,
    pub text: String,
    pub attachments: Vec<AttachmentRef>,
    pub event_time_ms: i64,
    pub labels: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboundEventKind {
    Created,
}

/// A complete, normalized inbound event ready for the Talon protocol adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundEvent {
    pub event_id: String,
    pub kind: InboundEventKind,
    pub binding: ConnectorBinding,
    pub message: NormalizedInboundMessage,
}

/// A request to deliver an agent response to a provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryRequest {
    pub delivery_id: String,
    pub binding: ConnectorBinding,
    pub namespace: String,
    pub connector_name: String,
    pub external_conversation_id: String,
    pub external_thread_id: Option<String>,
    pub reply_to_external_message_id: Option<String>,
    pub text: String,
    pub attachments: Vec<AttachmentRef>,
    pub labels: BTreeMap<String, String>,
}

/// A provider's non-secret delivery acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryReceipt {
    pub delivery_id: String,
    pub accepted: bool,
}

/// A request for provider-visible activity such as typing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityRequest {
    pub activity_id: String,
    pub binding: ConnectorBinding,
    pub namespace: String,
    pub connector_name: String,
    pub external_conversation_id: String,
    pub external_thread_id: Option<String>,
    pub kind: String,
    pub phase: String,
    pub status_text: String,
}

/// A provider's non-secret activity acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityReceipt {
    pub activity_id: String,
    pub accepted: bool,
}

/// Opaque secret bytes that are deliberately redacted from diagnostics.
#[derive(Eq, PartialEq)]
pub struct SecretMaterial(Vec<u8>);

impl SecretMaterial {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Uses the material without exposing a cloneable reference from this API.
    /// Callers remain responsible for not copying it into logs or long-lived
    /// provider state.
    pub fn with_bytes<T>(&self, use_bytes: impl FnOnce(&[u8]) -> T) -> T {
        use_bytes(&self.0)
    }
}

impl fmt::Debug for SecretMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretMaterial([REDACTED])")
    }
}

/// Stable, non-secret error categories used by connector contracts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectorError {
    InvalidRequest,
    ReplayRejected,
    Unavailable,
    Rejected,
    Internal,
}

impl fmt::Display for ConnectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            Self::InvalidRequest => "invalid_request",
            Self::ReplayRejected => "replay_rejected",
            Self::Unavailable => "unavailable",
            Self::Rejected => "rejected",
            Self::Internal => "internal",
        };
        formatter.write_str(code)
    }
}

impl std::error::Error for ConnectorError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_request_debug_redacts_body_and_header_values() {
        let request = RawInboundRequest::new(
            vec![("authorization".to_string(), "secret-header".to_string())],
            b"secret-body".to_vec(),
        );
        let debug = format!("{request:?}");
        assert!(debug.contains("header_count"));
        assert!(!debug.contains("authorization"));
        assert!(!debug.contains("secret-header"));
        assert!(!debug.contains("secret-body"));
    }

    #[test]
    fn raw_request_folds_header_names_and_preserves_duplicates() {
        let request = RawInboundRequest::new(
            vec![
                ("X-Signature".to_string(), "first".to_string()),
                ("x-signature".to_string(), "second".to_string()),
            ],
            Vec::new(),
        );
        assert_eq!(
            request.header_values("X-SIGNATURE"),
            Some(["first".to_string(), "second".to_string()].as_slice())
        );
    }

    #[test]
    fn secret_material_debug_and_errors_are_safe_to_log() {
        let secret = SecretMaterial::new(b"secret-value".to_vec());
        assert_eq!(format!("{secret:?}"), "SecretMaterial([REDACTED])");
        assert_eq!(
            ConnectorError::InvalidRequest.to_string(),
            "invalid_request"
        );
    }
}
