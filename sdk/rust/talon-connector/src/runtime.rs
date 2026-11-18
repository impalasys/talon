// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::{
    ActivityReceipt, ActivityRequest, AttachmentStore, Clock, ConnectorBinding,
    ConnectorDescriptor, ConnectorError, DeliveryReceipt, DeliveryRequest, EventSink, InboundEvent,
    InboundEventKind, NormalizedInboundMessage, RawInboundRequest, ReplayGuard, ReplayReservation,
    SecretMaterial, VerifiedInboundRequest,
};
use async_trait::async_trait;

/// Provider-neutral behavior implemented by a connector adapter.
#[async_trait]
pub trait Connector: Send + Sync {
    fn descriptor(&self) -> ConnectorDescriptor;

    /// Authenticates a provider request and returns a stable event identifier.
    ///
    /// Implementations receive only connector-scoped verification material.
    /// They must not submit events or retain secret material.
    async fn verify(
        &self,
        request: RawInboundRequest,
        verification: &VerificationContext<'_>,
        clock: &dyn Clock,
    ) -> Result<VerifiedInboundRequest, ConnectorError>;

    /// Converts a verified request into the provider-neutral inbound model.
    async fn ingest(
        &self,
        request: VerifiedInboundRequest,
        context: &ConnectorContext<'_>,
    ) -> Result<NormalizedInboundMessage, ConnectorError>;

    async fn deliver(
        &self,
        request: DeliveryRequest,
        context: &ConnectorContext<'_>,
    ) -> Result<DeliveryReceipt, ConnectorError>;

    async fn activity(
        &self,
        request: ActivityRequest,
        context: &ConnectorContext<'_>,
    ) -> Result<ActivityReceipt, ConnectorError>;
}

/// Host-selected capabilities available to post-verification connector work.
pub struct ConnectorContext<'a> {
    /// Provider configuration is selected and scoped by the host.
    pub provider_config: Option<&'a SecretMaterial>,
    pub attachments: &'a dyn AttachmentStore,
}

/// Connector-scoped material used only during provider request verification.
///
/// The host selects this value before it invokes a provider adapter; adapters
/// cannot request arbitrary host secrets through this API.
pub struct VerificationContext<'a> {
    pub secret: &'a SecretMaterial,
}

/// Owns generic inbound sequencing and is the only component that submits to
/// the event sink.
pub struct ConnectorHost<'a> {
    connector: &'a dyn Connector,
    verification: VerificationContext<'a>,
    binding: ConnectorBinding,
    clock: &'a dyn Clock,
    replay_guard: &'a dyn ReplayGuard,
    event_sink: &'a dyn EventSink,
    context: ConnectorContext<'a>,
}

impl<'a> ConnectorHost<'a> {
    pub fn new(
        connector: &'a dyn Connector,
        verification: VerificationContext<'a>,
        binding: ConnectorBinding,
        clock: &'a dyn Clock,
        replay_guard: &'a dyn ReplayGuard,
        event_sink: &'a dyn EventSink,
        context: ConnectorContext<'a>,
    ) -> Self {
        Self {
            connector,
            verification,
            binding,
            clock,
            replay_guard,
            event_sink,
            context,
        }
    }

    pub async fn handle_inbound(
        &self,
        request: RawInboundRequest,
    ) -> Result<InboundDisposition, ConnectorError> {
        let verified = self
            .connector
            .verify(request, &self.verification, self.clock)
            .await?;
        if self.replay_guard.reserve(&verified.event_id).await? == ReplayReservation::Duplicate {
            return Ok(InboundDisposition::Duplicate);
        }
        let event_id = verified.event_id.clone();
        let message = match self.connector.ingest(verified, &self.context).await {
            Ok(message) => message,
            Err(error) => {
                self.replay_guard.release(&event_id).await?;
                return Err(error);
            }
        };
        let event = InboundEvent {
            event_id: event_id.clone(),
            kind: InboundEventKind::Created,
            binding: self.binding.clone(),
            message,
        };
        if let Err(error) = self.event_sink.submit(event).await {
            self.replay_guard.release(&event_id).await?;
            return Err(error);
        }
        Ok(InboundDisposition::Accepted)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboundDisposition {
    Accepted,
    Duplicate,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FixedClock, InMemoryReplayGuard, RecordingEventSink};
    use crate::{AttachmentStore, ConnectorCapability, SecretMaterial, Sender};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::UNIX_EPOCH;

    struct TestConnector {
        reject: bool,
        fail_ingest_once: AtomicBool,
        ingest_calls: AtomicUsize,
    }

    #[async_trait]
    impl Connector for TestConnector {
        fn descriptor(&self) -> ConnectorDescriptor {
            ConnectorDescriptor {
                platform: "test".to_string(),
                capabilities: vec![ConnectorCapability::Ingest],
            }
        }

        async fn verify(
            &self,
            request: RawInboundRequest,
            _verification: &VerificationContext<'_>,
            _clock: &dyn Clock,
        ) -> Result<VerifiedInboundRequest, ConnectorError> {
            if self.reject {
                return Err(ConnectorError::InvalidRequest);
            }
            Ok(VerifiedInboundRequest::new("event-1".to_string(), request))
        }

        async fn ingest(
            &self,
            _request: VerifiedInboundRequest,
            _context: &ConnectorContext<'_>,
        ) -> Result<NormalizedInboundMessage, ConnectorError> {
            self.ingest_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_ingest_once.swap(false, Ordering::SeqCst) {
                return Err(ConnectorError::Unavailable);
            }
            Ok(NormalizedInboundMessage {
                external_message_id: "message-1".to_string(),
                external_conversation_id: "conversation-1".to_string(),
                external_thread_id: None,
                conversation_type: "dm".to_string(),
                sender: Sender {
                    id: "sender-1".to_string(),
                    display_name: None,
                },
                text: "hello".to_string(),
                attachments: Vec::new(),
                event_time_ms: 0,
                labels: BTreeMap::new(),
            })
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

    struct EmptyPorts;

    #[async_trait]
    impl AttachmentStore for EmptyPorts {
        async fn store(&self, _content: Vec<u8>) -> Result<String, ConnectorError> {
            Err(ConnectorError::Rejected)
        }
    }

    fn raw_request() -> RawInboundRequest {
        RawInboundRequest::new(Vec::new(), b"synthetic".to_vec())
    }

    fn context(ports: &EmptyPorts) -> ConnectorContext<'_> {
        ConnectorContext {
            provider_config: None,
            attachments: ports,
        }
    }

    fn binding() -> ConnectorBinding {
        ConnectorBinding {
            registration_id: "registration-1".to_string(),
            connector_class: "test".to_string(),
            match_fields: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn rejected_verification_does_not_ingest_or_submit() {
        let connector = TestConnector {
            reject: true,
            fail_ingest_once: AtomicBool::new(false),
            ingest_calls: AtomicUsize::new(0),
        };
        let ports = EmptyPorts;
        let secret = SecretMaterial::new(b"test-secret".to_vec());
        let clock = FixedClock(UNIX_EPOCH);
        let replay_guard = InMemoryReplayGuard::default();
        let event_sink = RecordingEventSink::default();
        let host = ConnectorHost::new(
            &connector,
            VerificationContext { secret: &secret },
            binding(),
            &clock,
            &replay_guard,
            &event_sink,
            context(&ports),
        );

        assert_eq!(
            host.handle_inbound(raw_request()).await,
            Err(ConnectorError::InvalidRequest)
        );
        assert_eq!(connector.ingest_calls.load(Ordering::SeqCst), 0);
        assert!(event_sink.events().is_empty());
    }

    #[tokio::test]
    async fn duplicate_event_is_not_ingested_or_submitted_twice() {
        let connector = TestConnector {
            reject: false,
            fail_ingest_once: AtomicBool::new(false),
            ingest_calls: AtomicUsize::new(0),
        };
        let ports = EmptyPorts;
        let secret = SecretMaterial::new(b"test-secret".to_vec());
        let clock = FixedClock(UNIX_EPOCH);
        let replay_guard = InMemoryReplayGuard::default();
        let event_sink = RecordingEventSink::default();
        let host = ConnectorHost::new(
            &connector,
            VerificationContext { secret: &secret },
            binding(),
            &clock,
            &replay_guard,
            &event_sink,
            context(&ports),
        );

        assert_eq!(
            host.handle_inbound(raw_request()).await,
            Ok(InboundDisposition::Accepted)
        );
        assert_eq!(
            host.handle_inbound(raw_request()).await,
            Ok(InboundDisposition::Duplicate)
        );
        assert_eq!(connector.ingest_calls.load(Ordering::SeqCst), 1);
        assert_eq!(event_sink.events().len(), 1);
    }

    #[tokio::test]
    async fn ingest_failure_releases_the_event_for_retry() {
        let connector = TestConnector {
            reject: false,
            fail_ingest_once: AtomicBool::new(true),
            ingest_calls: AtomicUsize::new(0),
        };
        let ports = EmptyPorts;
        let secret = SecretMaterial::new(b"test-secret".to_vec());
        let clock = FixedClock(UNIX_EPOCH);
        let replay_guard = InMemoryReplayGuard::default();
        let event_sink = RecordingEventSink::default();
        let host = ConnectorHost::new(
            &connector,
            VerificationContext { secret: &secret },
            binding(),
            &clock,
            &replay_guard,
            &event_sink,
            context(&ports),
        );

        assert_eq!(
            host.handle_inbound(raw_request()).await,
            Err(ConnectorError::Unavailable)
        );
        assert_eq!(
            host.handle_inbound(raw_request()).await,
            Ok(InboundDisposition::Accepted)
        );
        assert_eq!(connector.ingest_calls.load(Ordering::SeqCst), 2);
        assert_eq!(event_sink.events().len(), 1);
    }

    struct FailingOnceEventSink {
        fail_once: AtomicBool,
        events: Mutex<Vec<InboundEvent>>,
    }

    #[async_trait]
    impl EventSink for FailingOnceEventSink {
        async fn submit(&self, event: InboundEvent) -> Result<(), ConnectorError> {
            if self.fail_once.swap(false, Ordering::SeqCst) {
                return Err(ConnectorError::Unavailable);
            }
            self.events
                .lock()
                .expect("event sink lock poisoned")
                .push(event);
            Ok(())
        }
    }

    #[tokio::test]
    async fn event_sink_failure_releases_the_event_for_retry() {
        let connector = TestConnector {
            reject: false,
            fail_ingest_once: AtomicBool::new(false),
            ingest_calls: AtomicUsize::new(0),
        };
        let ports = EmptyPorts;
        let secret = SecretMaterial::new(b"test-secret".to_vec());
        let clock = FixedClock(UNIX_EPOCH);
        let replay_guard = InMemoryReplayGuard::default();
        let event_sink = FailingOnceEventSink {
            fail_once: AtomicBool::new(true),
            events: Mutex::new(Vec::new()),
        };
        let host = ConnectorHost::new(
            &connector,
            VerificationContext { secret: &secret },
            binding(),
            &clock,
            &replay_guard,
            &event_sink,
            context(&ports),
        );

        assert_eq!(
            host.handle_inbound(raw_request()).await,
            Err(ConnectorError::Unavailable)
        );
        assert_eq!(
            host.handle_inbound(raw_request()).await,
            Ok(InboundDisposition::Accepted)
        );
        assert_eq!(connector.ingest_calls.load(Ordering::SeqCst), 2);
        assert_eq!(event_sink.events.lock().unwrap().len(), 1);
        assert_eq!(
            event_sink.events.lock().unwrap()[0].binding.registration_id,
            "registration-1"
        );
    }
}
