// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{anyhow, Result};
use futures::FutureExt;
use std::future::Future;
use std::panic::{resume_unwind, AssertUnwindSafe};
use std::time::Duration;

use super::WorkerEventHandler;
use crate::control::ProtoKeyValueStoreExt;
use crate::gateway::rpc::connectors as connector_rpc;
use crate::gateway::rpc::data_proto;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub(super) const CONNECTOR_TYPING_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(20);

const LABEL_MESSAGE_SOURCE: &str = "talon.impalasys.com/message-source";
const LABEL_CONNECTOR_REGISTRATION: &str = "talon.impalasys.com/connector-registration";
const LABEL_CHANNEL_TRIGGER: &str = "talon.impalasys.com/channel-trigger";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ConnectorTypingActivity {
    pub(super) activity_id: String,
    pub(super) phase: &'static str,
    pub(super) status_text: &'static str,
}

impl ConnectorTypingActivity {
    fn start(submission_id: &str) -> Self {
        Self {
            activity_id: format!("{submission_id}:typing:start"),
            phase: "start",
            status_text: "is thinking...",
        }
    }

    fn active() -> Self {
        Self {
            activity_id: uuid::Uuid::now_v7().to_string(),
            phase: "active",
            status_text: "is thinking...",
        }
    }

    fn stop(submission_id: &str) -> Self {
        Self {
            activity_id: format!("{submission_id}:typing:stop"),
            phase: "stop",
            status_text: "",
        }
    }
}

pub(super) enum ConnectorTypingDelivery {
    Ineligible,
    Eligible(Result<()>),
}

enum ConnectorTypingActivityOutcome {
    Ineligible,
    Continue,
    Retry,
}

#[derive(Clone)]
pub(super) struct ConnectorTypingLogContext {
    pub(super) agent: String,
    pub(super) session_id: String,
    pub(super) submission_id: String,
}

pub(super) struct ConnectorTypingKeepalive {
    context: ConnectorTypingLogContext,
    heartbeat_cancellation: CancellationToken,
    heartbeat_task: Option<JoinHandle<()>>,
    stop_signal: Option<oneshot::Sender<()>>,
    stop_task: Option<JoinHandle<()>>,
}

async fn send_connector_typing_activity_best_effort<S, SFut>(
    sender: &S,
    context: &ConnectorTypingLogContext,
    activity: ConnectorTypingActivity,
) -> ConnectorTypingActivityOutcome
where
    S: Fn(ConnectorTypingActivity) -> SFut,
    SFut: Future<Output = Result<ConnectorTypingDelivery>>,
{
    let activity_id = activity.activity_id.clone();
    let phase = activity.phase;
    match sender(activity).await {
        Ok(ConnectorTypingDelivery::Ineligible) => ConnectorTypingActivityOutcome::Ineligible,
        Ok(ConnectorTypingDelivery::Eligible(Ok(()))) => ConnectorTypingActivityOutcome::Continue,
        Ok(ConnectorTypingDelivery::Eligible(Err(error))) => {
            tracing::warn!(
                error = %error,
                agent = %context.agent,
                session = %context.session_id,
                submission = %context.submission_id,
                activity_id = %activity_id,
                phase,
                "failed to send connector typing activity"
            );
            // Once a session reaches the sender, an endpoint failure must not
            // prevent later heartbeats or the final stop attempt.
            ConnectorTypingActivityOutcome::Continue
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                agent = %context.agent,
                session = %context.session_id,
                submission = %context.submission_id,
                activity_id = %activity_id,
                phase,
                "failed to resolve connector typing activity routing"
            );
            // A routing failure is not proof that the session is ineligible.
            // Keep the lifecycle alive and retry the next scheduled heartbeat.
            ConnectorTypingActivityOutcome::Retry
        }
    }
}

async fn run_connector_typing_keepalive<S, SFut>(
    cancellation: CancellationToken,
    interval: Duration,
    context: ConnectorTypingLogContext,
    sender: S,
    heartbeat_finished: oneshot::Sender<()>,
) where
    S: Fn(ConnectorTypingActivity) -> SFut + Send + Sync + 'static,
    SFut: Future<Output = Result<ConnectorTypingDelivery>> + Send + 'static,
{
    let first_tick = tokio::time::Instant::now() + interval;
    let mut ticks = tokio::time::interval_at(first_tick, interval);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            _ = ticks.tick() => {
                let activity = ConnectorTypingActivity::active();
                let send = send_connector_typing_activity_best_effort(
                    &sender,
                    &context,
                    activity,
                );
                tokio::pin!(send);
                let outcome = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => {
                        // Do not abandon an active request after it may have
                        // reached the connector. Stop delivery must wait for
                        // this request to settle to preserve activity order.
                        let _ = send.await;
                        ConnectorTypingActivityOutcome::Ineligible
                    },
                    outcome = &mut send => outcome
                };
                if matches!(outcome, ConnectorTypingActivityOutcome::Ineligible) {
                    break;
                }
            }
        }
    }

    let _ = heartbeat_finished.send(());
}

async fn run_connector_typing_stop<S, SFut>(
    context: ConnectorTypingLogContext,
    sender: S,
    stop_signal: oneshot::Receiver<()>,
    heartbeat_finished: oneshot::Receiver<()>,
) where
    S: Fn(ConnectorTypingActivity) -> SFut + Send + Sync + 'static,
    SFut: Future<Output = Result<ConnectorTypingDelivery>> + Send + 'static,
{
    if stop_signal.await.is_ok() {
        // The heartbeat task drains any request already in flight before it
        // signals completion. This prevents a late active activity from
        // arriving after stop.
        let _ = heartbeat_finished.await;
        let _ = send_connector_typing_activity_best_effort(
            &sender,
            &context,
            ConnectorTypingActivity::stop(&context.submission_id),
        )
        .await;
    }
}

async fn start_connector_typing_keepalive<S, SFut>(
    context: ConnectorTypingLogContext,
    interval: Duration,
    sender: S,
) -> Option<ConnectorTypingKeepalive>
where
    S: Fn(ConnectorTypingActivity) -> SFut + Clone + Send + Sync + 'static,
    SFut: Future<Output = Result<ConnectorTypingDelivery>> + Send + 'static,
{
    let start_outcome = send_connector_typing_activity_best_effort(
        &sender,
        &context,
        ConnectorTypingActivity::start(&context.submission_id),
    )
    .await;
    if !matches!(start_outcome, ConnectorTypingActivityOutcome::Continue) {
        return None;
    }

    let heartbeat_cancellation = CancellationToken::new();
    let (heartbeat_finished, heartbeat_finished_receiver) = oneshot::channel();
    let heartbeat_task = tokio::spawn(run_connector_typing_keepalive(
        heartbeat_cancellation.clone(),
        interval,
        context.clone(),
        sender.clone(),
        heartbeat_finished,
    ));
    let (stop_signal, stop_receiver) = oneshot::channel();
    let stop_task = tokio::spawn(run_connector_typing_stop(
        context.clone(),
        sender,
        stop_receiver,
        heartbeat_finished_receiver,
    ));

    Some(ConnectorTypingKeepalive {
        context,
        heartbeat_cancellation,
        heartbeat_task: Some(heartbeat_task),
        stop_signal: Some(stop_signal),
        stop_task: Some(stop_task),
    })
}

impl ConnectorTypingKeepalive {
    async fn cancel_and_join_heartbeat(&mut self) {
        self.heartbeat_cancellation.cancel();
        if let Some(task) = self.heartbeat_task.take() {
            if let Err(error) = task.await {
                tracing::warn!(
                    error = %error,
                    agent = %self.context.agent,
                    session = %self.context.session_id,
                    submission = %self.context.submission_id,
                    "connector typing keepalive task failed"
                );
            }
        }
    }

    pub(super) async fn stop_after_release(mut self) {
        self.cancel_and_join_heartbeat().await;

        let Some(stop_signal) = self.stop_signal.take() else {
            return;
        };
        if stop_signal.send(()).is_err() {
            return;
        }

        if let Some(task) = self.stop_task.take() {
            if let Err(error) = task.await {
                tracing::warn!(
                    error = %error,
                    agent = %self.context.agent,
                    session = %self.context.session_id,
                    submission = %self.context.submission_id,
                    "connector typing stop task failed"
                );
            }
        }
    }
}

impl Drop for ConnectorTypingKeepalive {
    fn drop(&mut self) {
        self.heartbeat_cancellation.cancel();
        if let Some(stop_signal) = self.stop_signal.take() {
            let _ = stop_signal.send(());
        }
    }
}

pub(super) async fn with_connector_typing_keepalive<S, SFut, F, T>(
    context: ConnectorTypingLogContext,
    interval: Duration,
    sender: S,
    work: F,
) -> (T, Option<ConnectorTypingKeepalive>)
where
    S: Fn(ConnectorTypingActivity) -> SFut + Clone + Send + Sync + 'static,
    SFut: Future<Output = Result<ConnectorTypingDelivery>> + Send + 'static,
    F: Future<Output = T>,
{
    let Some(mut keepalive) = start_connector_typing_keepalive(context, interval, sender).await
    else {
        return (work.await, None);
    };

    let outcome = AssertUnwindSafe(work).catch_unwind().await;
    keepalive.cancel_and_join_heartbeat().await;

    match outcome {
        Ok(value) => (value, Some(keepalive)),
        Err(panic) => {
            keepalive.stop_after_release().await;
            resume_unwind(panic);
        }
    }
}

impl WorkerEventHandler {
    pub(super) async fn maybe_send_connector_session_activity(
        &self,
        ns: &str,
        agent: &str,
        session_id: &str,
        activity_id: &str,
        phase: &str,
        status_text: &str,
    ) -> Result<ConnectorTypingDelivery> {
        let session = self
            .cp
            .kv
            .get_msg::<data_proto::Session>(&crate::control::keys::session(ns, agent, session_id))
            .await?
            .ok_or_else(|| anyhow!("session not found"))?;
        if !session.labels.contains_key(LABEL_CONNECTOR_REGISTRATION) {
            return Ok(ConnectorTypingDelivery::Ineligible);
        }
        if session
            .labels
            .get(LABEL_MESSAGE_SOURCE)
            .is_some_and(|source| source != "connector")
        {
            return Ok(ConnectorTypingDelivery::Ineligible);
        }
        if session.labels.contains_key(LABEL_CHANNEL_TRIGGER) {
            return Ok(ConnectorTypingDelivery::Ineligible);
        }
        Ok(ConnectorTypingDelivery::Eligible(
            connector_rpc::send_connector_session_activity(
                &self.cp,
                &session,
                activity_id,
                phase,
                status_text,
            )
            .await,
        ))
    }

    pub(super) async fn maybe_deliver_connector_session_reply(
        &self,
        ns: &str,
        agent: &str,
        session_id: &str,
        message_id: &str,
    ) -> Result<()> {
        connector_rpc::maybe_deliver_connector_session_message(
            &self.cp, ns, agent, session_id, message_id,
        )
        .await
    }
}
