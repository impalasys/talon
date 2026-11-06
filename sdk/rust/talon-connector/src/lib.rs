// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Provider-neutral, framework-free contracts for Talon connector runtimes.
//!
//! This crate intentionally contains no HTTP server, database, cloud identity,
//! provider SDK, or Talon server implementation. A host supplies the ports
//! required by a connector; a later crate will adapt normalized events to
//! Talon's public connector protocol.

mod model;
mod ports;
mod runtime;

pub use model::{
    ActivityReceipt, ActivityRequest, AttachmentRef, ConnectorBinding, ConnectorCapability,
    ConnectorDescriptor, ConnectorError, DeliveryReceipt, DeliveryRequest, InboundEvent,
    InboundEventKind, NormalizedInboundMessage, RawInboundRequest, SecretMaterial, Sender,
    VerifiedInboundRequest,
};
pub use ports::{AttachmentStore, Clock, EventSink, ReplayGuard, ReplayReservation};
pub use runtime::{
    Connector, ConnectorContext, ConnectorHost, InboundDisposition, VerificationContext,
};

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
