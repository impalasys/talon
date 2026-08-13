// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! Durable session-scoped active Skill state.

use anyhow::{anyhow, Result};
use prost::Message;

const CAS_RETRIES: usize = 8;
const ACTIVE_SKILLS_METADATA_KEY: &str = "talon.impalasys.com/active-skills";
const ACTIVE_SKILL_CONTEXT_DIGEST_METADATA_KEY: &str =
    "talon.impalasys.com/active-skill-context-digest";

pub async fn active_skill_names(
    kv: &dyn crate::control::KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
) -> Result<Vec<String>> {
    let key = crate::control::keys::session(ns, agent, session_id);
    let Some(bytes) = kv.get(&key).await? else {
        return Ok(Vec::new());
    };
    let session = crate::gateway::rpc::data_proto::Session::decode(bytes.as_slice())?;
    Ok(decode_active_skill_names(&session.metadata))
}

pub async fn activate_skill(
    kv: &dyn crate::control::KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    name: &str,
) -> Result<()> {
    mutate_active_skills(kv, ns, agent, session_id, |skills| {
        skills.retain(|skill| skill != name);
        skills.push(name.to_string());
        true
    })
    .await
    .map(|_| ())
}

pub async fn deactivate_skill(
    kv: &dyn crate::control::KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    name: &str,
) -> Result<bool> {
    mutate_active_skills(kv, ns, agent, session_id, |skills| {
        let before = skills.len();
        skills.retain(|skill| skill != name);
        skills.len() != before
    })
    .await
}

/// Persist the context digest and invalidate a remote provider continuation
/// whenever live package contents or active identities change.
pub async fn persist_active_skill_context_digest(
    kv: &dyn crate::control::KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    digest: &str,
) -> Result<bool> {
    let key = crate::control::keys::session(ns, agent, session_id);
    for _ in 0..CAS_RETRIES {
        let Some(bytes) = kv.get(&key).await? else {
            return Ok(false);
        };
        let mut session = crate::gateway::rpc::data_proto::Session::decode(bytes.as_slice())?;
        if session
            .metadata
            .get(ACTIVE_SKILL_CONTEXT_DIGEST_METADATA_KEY)
            .map(String::as_str)
            == Some(digest)
        {
            return Ok(false);
        }
        session.metadata.insert(
            ACTIVE_SKILL_CONTEXT_DIGEST_METADATA_KEY.to_string(),
            digest.to_string(),
        );
        if let Some(counter) = session.context_tokens.as_mut() {
            counter.provider_request_id = None;
        }
        if kv
            .compare_and_swap(&key, Some(bytes.as_slice()), &session.encode_to_vec())
            .await?
        {
            return Ok(true);
        }
    }
    Err(anyhow!(
        "failed to persist active Skill context digest after CAS retries"
    ))
}

async fn mutate_active_skills<F>(
    kv: &dyn crate::control::KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    mutate: F,
) -> Result<bool>
where
    F: Fn(&mut Vec<String>) -> bool,
{
    let key = crate::control::keys::session(ns, agent, session_id);
    for _ in 0..CAS_RETRIES {
        let bytes = kv
            .get(&key)
            .await?
            .ok_or_else(|| anyhow!("session not found"))?;
        let mut session = crate::gateway::rpc::data_proto::Session::decode(bytes.as_slice())?;
        let mut active_skills = decode_active_skill_names(&session.metadata);
        let changed = mutate(&mut active_skills);
        if !changed {
            return Ok(false);
        }
        session.metadata.insert(
            ACTIVE_SKILLS_METADATA_KEY.to_string(),
            serde_json::to_string(&active_skills).expect("active Skill names should serialize"),
        );
        session
            .metadata
            .remove(ACTIVE_SKILL_CONTEXT_DIGEST_METADATA_KEY);
        if let Some(counter) = session.context_tokens.as_mut() {
            counter.provider_request_id = None;
        }
        if kv
            .compare_and_swap(&key, Some(bytes.as_slice()), &session.encode_to_vec())
            .await?
        {
            return Ok(true);
        }
    }
    Err(anyhow!("failed to update active Skills after CAS retries"))
}

fn decode_active_skill_names(metadata: &std::collections::HashMap<String, String>) -> Vec<String> {
    metadata
        .get(ACTIVE_SKILLS_METADATA_KEY)
        .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{keys, ProtoKeyValueStoreExt};
    use crate::gateway::rpc::data_proto::{self, TokenCounter};
    use crate::test_support::MockKvStore;
    use std::sync::Arc;

    async fn seed_session(kv: &MockKvStore) {
        kv.set_msg(
            &keys::session("ns", "agent", "session"),
            &data_proto::Session {
                id: "session".to_string(),
                agent: "agent".to_string(),
                ns: "ns".to_string(),
                status: "IDLE".to_string(),
                created_at: 1,
                last_active: 2,
                metadata: [("unrelated".to_string(), "preserved".to_string())]
                    .into_iter()
                    .collect(),
                labels: Default::default(),
                context_tokens: Some(TokenCounter {
                    provider_request_id: Some("resp-1".to_string()),
                    ..Default::default()
                }),
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn activation_is_ordered_durable_and_invalidates_continuation() {
        let kv = Arc::new(MockKvStore::default());
        seed_session(kv.as_ref()).await;

        activate_skill(kv.as_ref(), "ns", "agent", "session", "review")
            .await
            .unwrap();
        activate_skill(kv.as_ref(), "ns", "agent", "session", "release")
            .await
            .unwrap();
        activate_skill(kv.as_ref(), "ns", "agent", "session", "review")
            .await
            .unwrap();

        assert_eq!(
            active_skill_names(kv.as_ref(), "ns", "agent", "session")
                .await
                .unwrap(),
            vec!["release", "review"]
        );
        let session = kv
            .get_msg::<data_proto::Session>(&keys::session("ns", "agent", "session"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            session.metadata.get("unrelated"),
            Some(&"preserved".to_string())
        );
        assert!(session
            .context_tokens
            .as_ref()
            .unwrap()
            .provider_request_id
            .is_none());
    }

    #[tokio::test]
    async fn deactivation_is_idempotent_and_digest_invalidates_once() {
        let kv = Arc::new(MockKvStore::default());
        seed_session(kv.as_ref()).await;
        activate_skill(kv.as_ref(), "ns", "agent", "session", "review")
            .await
            .unwrap();

        assert!(persist_active_skill_context_digest(
            kv.as_ref(),
            "ns",
            "agent",
            "session",
            "digest-1"
        )
        .await
        .unwrap());
        assert!(!persist_active_skill_context_digest(
            kv.as_ref(),
            "ns",
            "agent",
            "session",
            "digest-1"
        )
        .await
        .unwrap());
        assert!(
            deactivate_skill(kv.as_ref(), "ns", "agent", "session", "review")
                .await
                .unwrap()
        );
        assert!(
            !deactivate_skill(kv.as_ref(), "ns", "agent", "session", "review")
                .await
                .unwrap()
        );
    }
}
