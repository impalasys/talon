// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Server-owned admission order, separate from client message IDs/timestamps.
use crate::control::{keys, KeyValueStore, ProtoKeyValueStoreExt};
use crate::talon::protocol::data_proto;
use anyhow::{anyhow, Result};
use std::collections::HashMap;

fn counter_key(ns: &str, agent: &str, session: &str) -> keys::ResourceKey {
    keys::ResourceKey::new(
        ns,
        &[("Agent", agent), ("Session", session)],
        "MessageAdmissionCounter",
        "counter",
    )
}
fn admission_key(ns: &str, agent: &str, session: &str, message: &str) -> keys::ResourceKey {
    keys::ResourceKey::new(
        ns,
        &[("Agent", agent), ("Session", session)],
        "MessageAdmission",
        message,
    )
}
fn decode_order(bytes: &[u8]) -> Result<u64> {
    Ok(u64::from_be_bytes(bytes.try_into().map_err(|_| {
        anyhow!("invalid session message admission order")
    })?))
}

pub async fn current_message_admission_order(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
) -> Result<u64> {
    kv.get(&counter_key(ns, agent, session))
        .await?
        .map(|v| decode_order(&v))
        .transpose()
        .map(|v| v.unwrap_or(0))
}

/// Assign once per canonical User message ID. Neither edits nor duplicate
/// delivery move an old message past a confirmation's ordering boundary.
pub async fn record_user_message_admission(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    message: &data_proto::SessionMessage,
) -> Result<()> {
    if message.role != data_proto::MessageRole::RoleUser as i32 {
        return Ok(());
    }
    let key = admission_key(ns, agent, session, &message.id);
    if kv.get(&key).await?.is_some() {
        return Ok(());
    }
    let counter = counter_key(ns, agent, session);
    for _ in 0..32 {
        let current = kv.get(&counter).await?;
        let next = current
            .as_deref()
            .map(decode_order)
            .transpose()?
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| anyhow!("session admission order exhausted"))?;
        if kv
            .compare_and_swap(&counter, current.as_deref(), &next.to_be_bytes())
            .await?
        {
            // Gaps after failed writes or a duplicate race are harmless. An
            // already admitted ID must keep its original server order.
            kv.compare_and_swap(&key, None, &next.to_be_bytes()).await?;
            return Ok(());
        }
    }
    Err(anyhow!(
        "failed to allocate session message admission order"
    ))
}

pub async fn persist_admitted_session_message(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
    message: &data_proto::SessionMessage,
) -> Result<()> {
    record_user_message_admission(kv, ns, agent, session, message).await?;
    kv.set_msg(
        &keys::session_message(ns, agent, session, &message.id),
        message,
    )
    .await
}

pub async fn message_admission_orders(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session: &str,
) -> Result<HashMap<String, u64>> {
    let prefix = keys::ResourceParent::root(ns)
        .child("Agent", agent)
        .child("Session", session)
        .list(Some("MessageAdmission"));
    kv.list_entries(&prefix, None)
        .await?
        .into_iter()
        .map(|(key, value)| Ok((key.name, decode_order(&value)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockKvStore;
    #[tokio::test]
    async fn admission_order_ignores_timestamps_and_is_idempotent_under_concurrency() {
        let kv = MockKvStore::default();
        let mut first = data_proto::SessionMessage {
            id: "z-client-id".into(),
            role: data_proto::MessageRole::RoleUser as i32,
            created_at: i64::MAX,
            ..Default::default()
        };
        let second = data_proto::SessionMessage {
            id: "a-client-id".into(),
            role: data_proto::MessageRole::RoleUser as i32,
            created_at: 1,
            ..Default::default()
        };
        record_user_message_admission(&kv, "ns", "agent", "session", &first)
            .await
            .unwrap();
        let marker = current_message_admission_order(&kv, "ns", "agent", "session")
            .await
            .unwrap();
        let (a, b) = tokio::join!(
            record_user_message_admission(&kv, "ns", "agent", "session", &second),
            record_user_message_admission(&kv, "ns", "agent", "session", &second)
        );
        a.unwrap();
        b.unwrap();
        first.created_at = 0;
        record_user_message_admission(&kv, "ns", "agent", "session", &first)
            .await
            .unwrap();
        let orders = message_admission_orders(&kv, "ns", "agent", "session")
            .await
            .unwrap();
        assert_eq!(orders[&first.id], marker);
        assert!(orders[&second.id] > marker);
        assert_eq!(orders.len(), 2);
    }
}
