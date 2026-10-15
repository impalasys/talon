// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! A deliberately non-operational iMessage connector reference skeleton.
//!
//! This crate has no provider SDKs, network clients, local message-database
//! bindings, bridge integrations, credentials, or account setup. It exists to
//! demonstrate capability declaration and typed synthetic-message normalization
//! against `talon-connector`. A production runtime needs a separately approved
//! design covering platform compliance, deployment, permissions, and terms.

use async_trait::async_trait;
use std::collections::BTreeMap;
use talon_connector::{
    ActivityReceipt, ActivityRequest, AttachmentRef, Clock, Connector, ConnectorContext,
    ConnectorDescriptor, ConnectorError, DeliveryReceipt, DeliveryRequest,
    NormalizedInboundMessage, RawInboundRequest, Sender, VerificationContext,
    VerifiedInboundRequest,
};

/// Non-secret metadata for a reference skeleton instance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImessageReferenceConfig {
    pub display_name: String,
}

/// Public, typed input used only by synthetic tests and examples.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntheticImessageMessage {
    pub message_id: String,
    pub conversation_id: String,
    pub thread_id: Option<String>,
    pub sender_id: String,
    pub sender_display_name: Option<String>,
    pub text: String,
    pub event_time_ms: i64,
    pub attachments: Vec<AttachmentRef>,
}

/// A no-network, no-provider-API connector skeleton.
pub struct ImessageReferenceConnector {
    config: ImessageReferenceConfig,
}

impl ImessageReferenceConnector {
    pub fn new(config: ImessageReferenceConfig) -> Result<Self, ConnectorError> {
        if config.display_name.trim().is_empty() {
            return Err(ConnectorError::InvalidRequest);
        }
        Ok(Self { config })
    }

    pub fn config(&self) -> &ImessageReferenceConfig {
        &self.config
    }

    /// Converts a typed, synthetic iMessage-shaped value into the generic
    /// normalized model. It does not parse, read, or send provider traffic.
    pub fn normalize_synthetic(
        &self,
        message: SyntheticImessageMessage,
    ) -> Result<NormalizedInboundMessage, ConnectorError> {
        if message.message_id.trim().is_empty()
            || message.conversation_id.trim().is_empty()
            || message.sender_id.trim().is_empty()
        {
            return Err(ConnectorError::InvalidRequest);
        }
        Ok(NormalizedInboundMessage {
            external_message_id: message.message_id,
            external_conversation_id: message.conversation_id,
            external_thread_id: message.thread_id,
            conversation_type: "dm".to_string(),
            sender: Sender {
                id: message.sender_id,
                display_name: message.sender_display_name,
            },
            text: message.text,
            attachments: message.attachments,
            event_time_ms: message.event_time_ms,
            labels: BTreeMap::new(),
        })
    }
}

#[async_trait]
impl Connector for ImessageReferenceConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            platform: "imessage".to_string(),
            capabilities: Vec::new(),
        }
    }

    async fn verify(
        &self,
        _request: RawInboundRequest,
        _verification: &VerificationContext<'_>,
        _clock: &dyn Clock,
    ) -> Result<VerifiedInboundRequest, ConnectorError> {
        Err(ConnectorError::Rejected)
    }

    async fn ingest(
        &self,
        _request: VerifiedInboundRequest,
        _context: &ConnectorContext<'_>,
    ) -> Result<NormalizedInboundMessage, ConnectorError> {
        Err(ConnectorError::Rejected)
    }

    async fn deliver(
        &self,
        _request: DeliveryRequest,
        _context: &ConnectorContext<'_>,
    ) -> Result<DeliveryReceipt, ConnectorError> {
        Err(ConnectorError::Rejected)
    }

    async fn activity(
        &self,
        _request: ActivityRequest,
        _context: &ConnectorContext<'_>,
    ) -> Result<ActivityReceipt, ConnectorError> {
        Err(ConnectorError::Rejected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connector() -> ImessageReferenceConnector {
        ImessageReferenceConnector::new(ImessageReferenceConfig {
            display_name: "Synthetic test connector".to_string(),
        })
        .unwrap()
    }

    #[test]
    fn declares_no_operational_capabilities() {
        assert_eq!(
            connector().descriptor(),
            ConnectorDescriptor {
                platform: "imessage".to_string(),
                capabilities: Vec::new(),
            }
        );
    }

    #[test]
    fn normalizes_a_typed_synthetic_message() {
        let normalized = connector()
            .normalize_synthetic(SyntheticImessageMessage {
                message_id: "synthetic-message-1".to_string(),
                conversation_id: "synthetic-conversation-1".to_string(),
                thread_id: Some("synthetic-thread-1".to_string()),
                sender_id: "synthetic-sender-1".to_string(),
                sender_display_name: Some("Synthetic Sender".to_string()),
                text: "hello".to_string(),
                event_time_ms: 1,
                attachments: vec![AttachmentRef {
                    id: "synthetic-attachment-1".to_string(),
                    content_type: Some("text/plain".to_string()),
                }],
            })
            .unwrap();

        assert_eq!(
            normalized.external_conversation_id,
            "synthetic-conversation-1"
        );
        assert_eq!(
            normalized.external_thread_id.as_deref(),
            Some("synthetic-thread-1")
        );
        assert_eq!(
            normalized.sender.display_name.as_deref(),
            Some("Synthetic Sender")
        );
        assert_eq!(normalized.external_message_id, "synthetic-message-1");
        assert_eq!(normalized.conversation_type, "dm");
        assert_eq!(normalized.sender.id, "synthetic-sender-1");
        assert_eq!(normalized.text, "hello");
        assert_eq!(normalized.event_time_ms, 1);
        assert_eq!(
            normalized.attachments,
            vec![AttachmentRef {
                id: "synthetic-attachment-1".to_string(),
                content_type: Some("text/plain".to_string()),
            }]
        );
        assert!(normalized.labels.is_empty());
    }

    #[test]
    fn rejects_missing_non_secret_configuration_or_message_identity() {
        assert!(matches!(
            ImessageReferenceConnector::new(ImessageReferenceConfig {
                display_name: " ".to_string(),
            }),
            Err(ConnectorError::InvalidRequest)
        ));
        for (message_id, conversation_id, sender_id) in [
            ("", "conversation", "sender"),
            ("message", " ", "sender"),
            ("message", "conversation", ""),
        ] {
            assert_eq!(
                connector().normalize_synthetic(SyntheticImessageMessage {
                    message_id: message_id.to_string(),
                    conversation_id: conversation_id.to_string(),
                    thread_id: None,
                    sender_id: sender_id.to_string(),
                    sender_display_name: None,
                    text: String::new(),
                    event_time_ms: 0,
                    attachments: Vec::new(),
                }),
                Err(ConnectorError::InvalidRequest)
            );
        }
    }

    struct EmptyAttachments;

    #[async_trait]
    impl talon_connector::AttachmentStore for EmptyAttachments {
        async fn store(&self, _content: Vec<u8>) -> Result<String, ConnectorError> {
            Err(ConnectorError::Rejected)
        }
    }

    #[tokio::test]
    async fn connector_trait_methods_are_explicitly_non_operational() {
        let connector = connector();
        let attachments = EmptyAttachments;
        let secret = talon_connector::SecretMaterial::new(b"synthetic-secret".to_vec());
        let verification = VerificationContext { secret: &secret };
        let context = ConnectorContext {
            provider_config: None,
            attachments: &attachments,
        };
        let binding = talon_connector::ConnectorBinding {
            registration_id: "synthetic-registration".to_string(),
            connector_class: "imessage".to_string(),
            match_fields: BTreeMap::new(),
        };
        let raw = RawInboundRequest::new(Vec::new(), Vec::new());
        assert_eq!(
            connector
                .verify(raw.clone(), &verification, &FixedClock)
                .await,
            Err(ConnectorError::Rejected)
        );
        assert_eq!(
            connector
                .ingest(
                    VerifiedInboundRequest::new("synthetic-event".to_string(), raw),
                    &context,
                )
                .await,
            Err(ConnectorError::Rejected)
        );
        assert_eq!(
            connector
                .deliver(
                    DeliveryRequest {
                        delivery_id: "synthetic-delivery".to_string(),
                        binding: binding.clone(),
                        namespace: "synthetic-namespace".to_string(),
                        connector_name: "synthetic-connector".to_string(),
                        external_conversation_id: "synthetic-conversation".to_string(),
                        external_thread_id: None,
                        reply_to_external_message_id: None,
                        text: "hello".to_string(),
                        attachments: Vec::new(),
                        labels: BTreeMap::new(),
                    },
                    &context,
                )
                .await,
            Err(ConnectorError::Rejected)
        );
        assert_eq!(
            connector
                .activity(
                    ActivityRequest {
                        activity_id: "synthetic-activity".to_string(),
                        binding,
                        namespace: "synthetic-namespace".to_string(),
                        connector_name: "synthetic-connector".to_string(),
                        external_conversation_id: "synthetic-conversation".to_string(),
                        external_thread_id: None,
                        kind: "typing".to_string(),
                        phase: "start".to_string(),
                        status_text: String::new(),
                    },
                    &context,
                )
                .await,
            Err(ConnectorError::Rejected)
        );
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> std::time::SystemTime {
            std::time::UNIX_EPOCH
        }
    }
}
