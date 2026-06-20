// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use prost::Message;

use crate::control::{keys, KeyValueStore};
use crate::gateway::rpc::data_proto;

use super::{renew_submission_claim, RenewOutcome};

const MAX_SESSION_RENEW_CAS_RETRIES: usize = 8;

#[derive(Debug, Clone)]
pub struct SubmissionLease {
    pub ns: String,
    pub agent: String,
    pub session_id: String,
    pub submission_id: String,
    pub attempt_id: String,
    pub ttl_micros: i64,
}

pub struct SubmissionLeaseRenewer {
    handle: tokio::task::JoinHandle<()>,
    last_renewed_at: Arc<AtomicI64>,
}

impl SubmissionLeaseRenewer {
    pub fn start(
        kv: Arc<dyn KeyValueStore + Send + Sync>,
        lease: SubmissionLease,
        initial_renewed_at: i64,
    ) -> Self {
        let last_renewed_at = Arc::new(AtomicI64::new(initial_renewed_at));
        let task_last_renewed_at = last_renewed_at.clone();
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(lease_renewal_interval(lease.ttl_micros));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            interval.tick().await;

            loop {
                interval.tick().await;
                let now_micros = chrono::Utc::now().timestamp_micros();
                match renew_submission_claim(
                    kv.as_ref(),
                    &lease.ns,
                    &lease.agent,
                    &lease.session_id,
                    &lease.submission_id,
                    &lease.attempt_id,
                    now_micros,
                    lease.ttl_micros,
                )
                .await
                {
                    Ok(RenewOutcome::Renewed(submission)) => {
                        match renew_session_lock_timestamp(
                            kv.as_ref(),
                            &lease.ns,
                            &lease.agent,
                            &lease.session_id,
                            now_micros,
                        )
                        .await
                        {
                            Ok(()) => {
                                task_last_renewed_at.store(now_micros, Ordering::SeqCst);
                            }
                            Err(err) => {
                                tracing::warn!(
                                    namespace = %lease.ns,
                                    agent = %lease.agent,
                                    session = %lease.session_id,
                                    submission = %submission.submission_id,
                                    error = %err,
                                    "Failed to renew session processing lock"
                                );
                            }
                        }
                    }
                    Ok(RenewOutcome::NotCurrent(submission)) => {
                        tracing::info!(
                            namespace = %lease.ns,
                            agent = %lease.agent,
                            session = %lease.session_id,
                            submission = %lease.submission_id,
                            current_attempt = %submission.attempt_id,
                            expected_attempt = %lease.attempt_id,
                            "Stopping session lease renewal because claim is no longer current"
                        );
                        break;
                    }
                    Ok(RenewOutcome::AlreadyTerminal(_)) | Ok(RenewOutcome::Missing) => break,
                    Err(err) => {
                        tracing::warn!(
                            namespace = %lease.ns,
                            agent = %lease.agent,
                            session = %lease.session_id,
                            submission = %lease.submission_id,
                            error = %err,
                            "Failed to renew session submission claim"
                        );
                    }
                }
            }
        });

        Self {
            handle,
            last_renewed_at,
        }
    }

    pub fn last_renewed_at(&self) -> i64 {
        self.last_renewed_at.load(Ordering::SeqCst)
    }
}

impl Drop for SubmissionLeaseRenewer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn lease_renewal_interval(ttl_micros: i64) -> Duration {
    let ttl_micros = ttl_micros.max(1) as u64;
    Duration::from_micros((ttl_micros / 3).max(100_000))
}

async fn renew_session_lock_timestamp(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    now_micros: i64,
) -> Result<()> {
    let key = keys::session(ns, agent, session_id);
    for _ in 0..MAX_SESSION_RENEW_CAS_RETRIES {
        let Some(current) = kv.get(&key).await? else {
            return Ok(());
        };
        let mut session = data_proto::Session::decode(current.as_slice())?;
        if session.status != "PROCESSING" || session.last_active >= now_micros {
            return Ok(());
        }
        session.last_active = now_micros;
        let updated = session.encode_to_vec();
        if kv
            .compare_and_swap(&key, Some(current.as_slice()), &updated)
            .await?
        {
            return Ok(());
        }
    }
    anyhow::bail!("failed to atomically renew session processing lock")
}
