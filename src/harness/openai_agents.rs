// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

//! OpenAI owns execution and conversation state. The records below only track
//! delivery of Talon input and association of remote turns with local replies.

use anyhow::{anyhow, bail, Context, Result};
use futures::{stream::BoxStream, StreamExt};
use prost::Message;
use reqwest::{Client, Method, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

use crate::control::{config::Config, keys, ControlPlane, KeyValueStore, ProtoKeyValueStoreExt};
use crate::gateway::rpc::{data_proto, manifests};
use crate::harness::{llm::resolver, sessions};

type Events = BoxStream<'static, Result<Value>>;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const RECOVERY_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const ADMISSION_PREFIX: &str = "openai.admission.";

/// A separate monotonic record cannot be overwritten by an observer checkpoint.
pub(crate) async fn request_cancellation(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    submission_id: &str,
) -> Result<()> {
    kv.set(
        &keys::ResourceKey::new(
            ns,
            &[("Agent", agent), ("Session", session_id)],
            "OpenAICancel",
            submission_id,
        ),
        b"true",
    )
    .await
}

/// Reserve input in the same record that Clear and worker completion lock.
/// A nonempty token means child writes are still in progress; an empty token
/// means the input is durable. Never take over a writer based on elapsed time.
pub(crate) async fn admit_input(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
    now: i64,
) -> Result<String> {
    let key = keys::session(ns, agent, session_id);
    let token = crate::control::uuid::session_submission_id();
    for _ in 0..8 {
        let raw = kv
            .get(&key)
            .await?
            .ok_or(crate::control::scheduling::SessionNotFoundError)?;
        let mut session = data_proto::Session::decode(raw.as_slice())?;
        if session
            .metadata
            .contains_key(crate::control::scheduling::SESSION_CLEARING)
        {
            return Err(crate::control::scheduling::SessionCurrentlyProcessingError.into());
        }
        if session
            .metadata
            .get(&format!("{ADMISSION_PREFIX}{message_id}"))
            .is_some_and(|token| !token.is_empty())
        {
            return Err(RecoveryNeeded(format!(
                "Input {message_id} is still being saved. Retry after its writer finishes; if that gateway stopped, inspect this session and use a new session instead of taking over an unresolved write."
            )).into());
        }
        session.status = "PROCESSING".into();
        session.last_active = now.max(session.last_active.saturating_add(1));
        session
            .metadata
            .insert(format!("{ADMISSION_PREFIX}{message_id}"), token.clone());
        if kv
            .compare_and_swap(&key, Some(&raw), &session.encode_to_vec())
            .await?
        {
            #[cfg(test)]
            test_pause("input_reserved").await;
            return Ok(token);
        }
    }
    bail!("Could not reserve OpenAI input; retry this message ID")
}

pub(crate) async fn inputs_finished(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    session: &data_proto::Session,
) -> Result<bool> {
    if session
        .metadata
        .iter()
        .any(|(key, token)| key.starts_with(ADMISSION_PREFIX) && !token.is_empty())
    {
        return Ok(false);
    }
    let mut admitting: std::collections::HashSet<&str> = session
        .metadata
        .keys()
        .filter_map(|key| key.strip_prefix(ADMISSION_PREFIX))
        .collect();
    for (_, bytes) in kv
        .list_entries(
            &keys::session_submission_prefix(ns, agent, session_id),
            None,
        )
        .await?
    {
        let submission = data_proto::SessionSubmission::decode(bytes.as_slice())?;
        if !sessions::submission_is_terminal(&submission) {
            return Ok(false);
        }
        admitting.remove(submission.submission_id.as_str());
    }
    Ok(admitting.is_empty())
}

/// Only the writer may finish its reservation. Stored input remains protected
/// until its submission completes, including after a dispatch publish failure.
pub(crate) async fn finish_admission(
    kv: &dyn KeyValueStore,
    ns: &str,
    agent: &str,
    session_id: &str,
    message_id: &str,
    token: &str,
    saved: bool,
) -> Result<()> {
    let key = keys::session(ns, agent, session_id);
    let marker = format!("{ADMISSION_PREFIX}{message_id}");
    for _ in 0..8 {
        let Some(raw) = kv.get(&key).await? else {
            return Ok(());
        };
        let mut session = data_proto::Session::decode(raw.as_slice())?;
        if session.metadata.get(&marker).map(String::as_str) != Some(token) {
            return Ok(());
        }
        let mut durable = saved
            || kv
                .get(&keys::session_submission(ns, agent, session_id, message_id))
                .await?
                .is_some();
        if !durable {
            // A failed retry must also preserve an earlier connector queue entry.
            for queue in ["next", "steer", "a2a"] {
                durable |= kv
                    .list_keys(
                        &keys::session_queue_prefix(ns, agent, session_id, queue),
                        None,
                    )
                    .await?
                    .iter()
                    .any(|key| key.name.ends_with(&format!("-{message_id}")));
            }
        }
        if durable {
            session.metadata.insert(marker.clone(), String::new());
        } else {
            session.metadata.remove(&marker);
        }
        if inputs_finished(kv, ns, agent, session_id, &session).await? {
            session.status = "IDLE".into();
            session
                .metadata
                .retain(|key, _| !key.starts_with(ADMISSION_PREFIX));
        }
        if kv
            .compare_and_swap(&key, Some(&raw), &session.encode_to_vec())
            .await?
        {
            return Ok(());
        }
    }
    bail!("Could not finish OpenAI input reservation; inspect this session before retrying")
}

#[derive(Debug)]
pub struct RecoveryNeeded(pub String);
impl std::fmt::Display for RecoveryNeeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for RecoveryNeeded {}

#[derive(Debug)]
struct Rejected(String);
impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Rejected {}

pub fn is_openai_agents(spec: &manifests::AgentSpec) -> bool {
    spec.runtime
        .as_ref()
        .is_some_and(|r| r.kind == "openai_agents")
}

pub async fn uses_openai_agents(kv: &dyn KeyValueStore, ns: &str, agent: &str) -> Result<bool> {
    let Some(bytes) = kv.get(&keys::agent(ns, agent)).await? else {
        return Ok(false);
    };
    let resource =
        crate::control::resources::ResourceStore::decode_stored_resource("Agent", &bytes)?;
    Ok(matches!(resource.spec.and_then(|s| s.kind),
        Some(manifests::resource_spec::Kind::Agent(ref spec)) if is_openai_agents(spec)))
}

pub fn validate_spec(spec: &manifests::AgentSpec) -> Result<()> {
    if !spec.features.is_empty()
        || !spec.mcp_server_refs.is_empty()
        || !spec.capabilities.is_empty()
        || spec.a2a.is_some()
        || !spec.post_history_prompt.is_empty()
        || spec.runtime.as_ref().is_some_and(|r| r.acp.is_some())
    {
        bail!("openai_agents supports systemPrompt and plain text only; features, MCP, capabilities, A2A, postHistoryPrompt and ACP configuration are unsupported");
    }
    let model = resolver::resolve_model_profile(spec.model_policy.as_ref())
        .context("openai_agents requires a default model")?;
    if model.temperature != 0.0
        || model
            .thinking
            .as_ref()
            .is_some_and(|t| t.budget_tokens.is_some())
    {
        bail!("openai_agents does not support temperature or thinking.budgetTokens");
    }
    Ok(())
}

pub fn validate_message(message: &data_proto::SessionMessage) -> Result<()> {
    if !matches!(message.role, 0 | 1)
        || message.parts.is_empty()
        || message.parts.iter().any(|p| {
            p.part_type != data_proto::SessionMessagePartType::Text as i32
                || p.object.is_some()
                || !p.payload_json.is_empty()
        })
        || message.parts.iter().all(|p| p.content.trim().is_empty())
    {
        bail!("openai_agents accepts non-empty user text only; attachments and other message parts are unsupported");
    }
    Ok(())
}

/// Preserve the original input on transport retries with the same message ID.
pub async fn persist_input(
    kv: &dyn KeyValueStore,
    key: &keys::ResourceKey,
    message: &data_proto::SessionMessage,
) -> Result<()> {
    if !kv
        .compare_and_swap(key, None, &message.encode_to_vec())
        .await?
    {
        let saved = kv
            .get_msg::<data_proto::SessionMessage>(key)
            .await?
            .context("Stored OpenAI input is missing")?;
        if saved.role != message.role
            || saved.labels != message.labels
            || crate::control::scheduling::session_message_text_projection(&saved)
                != crate::control::scheduling::session_message_text_projection(message)
        {
            bail!("OpenAI message ID was already used for different input");
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PendingInput {
    submission_id: String,
    attempt_id: String,
    text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RemoteSession {
    nonce: String,
    configuration: String,
    session_id: Option<String>,
    pending: Option<PendingInput>,
    next_input: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcceptedInput {
    pub session_id: String,
    pub index: usize,
    pub text: String,
    pub item_id: String,
    pub turn_id: String,
    pub observed_turn_id: Option<String>,
    pub discarded: bool,
    #[serde(default)]
    pub cancel_requested: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnReply {
    pub submission_id: String,
    pub message_id: String,
}

pub struct TurnResult {
    pub status: String,
    pub text: String,
    pub error: Option<String>,
    pub reply: TurnReply,
}

pub struct OpenAiAgentRuntime {
    cp: Arc<ControlPlane>,
    client: Client,
    api_key: String,
    base_url: String,
    agent_config: Value,
    configuration: String,
    ns: String,
    agent: String,
    session: String,
}

impl OpenAiAgentRuntime {
    pub async fn build(
        cp: Arc<ControlPlane>,
        config: &Config,
        ns: &str,
        agent: &str,
        session: &str,
        spec: &manifests::AgentSpec,
    ) -> Result<Self> {
        validate_spec(spec)?;
        let (api_key, base_url, model) =
            resolver::resolve_openai_agents_credentials(spec, config, &cp, ns).await?;
        let mut agent_config = json!({"model":model,"instructions":spec.system_prompt});
        if let Some(thinking) = resolver::resolve_model_profile(spec.model_policy.as_ref())
            .and_then(|m| m.thinking.as_ref())
        {
            if !thinking.effort.is_empty() {
                agent_config["reasoning"] = json!({"effort":thinking.effort});
            } else if !thinking.enabled {
                agent_config["reasoning"] = json!({"effort":"none"});
            }
        }
        let configuration = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&json!({
                "agent":agent_config,"provider":resolver::resolve_model_profile(spec.model_policy.as_ref()).map(|m| &m.provider),
                "endpoint":base_url,
            }))?)
        );
        Ok(Self {
            cp,
            client: Client::builder().connect_timeout(REQUEST_TIMEOUT).build()?,
            api_key,
            base_url: base_url.trim_end_matches('/').into(),
            agent_config,
            configuration,
            ns: ns.into(),
            agent: agent.into(),
            session: session.into(),
        })
    }

    fn key(&self, kind: &str, name: &str) -> keys::ResourceKey {
        keys::ResourceKey::new(
            &self.ns,
            &[("Agent", &self.agent), ("Session", &self.session)],
            kind,
            name,
        )
    }

    async fn read<T: serde::de::DeserializeOwned>(
        &self,
        kind: &str,
        name: &str,
    ) -> Result<Option<T>> {
        self.cp
            .kv
            .get(&self.key(kind, name))
            .await?
            .map(|bytes| serde_json::from_slice(&bytes).map_err(Into::into))
            .transpose()
    }

    async fn current_attempt(&self, pending: &PendingInput) -> Result<bool> {
        Ok(self
            .cp
            .kv
            .get_msg::<data_proto::SessionSubmission>(&keys::session_submission(
                &self.ns,
                &self.agent,
                &self.session,
                &pending.submission_id,
            ))
            .await?
            .is_some_and(|s| {
                s.attempt_id == pending.attempt_id
                    && !sessions::submission_is_terminal(&s)
                    && s.claim_expires_at
                        .is_some_and(|t| t > chrono::Utc::now().timestamp_micros())
            }))
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}/agents/sessions{}", self.base_url, path))
            .bearer_auth(&self.api_key)
            .header("OpenAI-Beta", "agents=v1")
    }

    async fn checked(response: Response) -> Result<Response> {
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let message = format!(
                "OpenAI Agents API HTTP {status}: {}",
                body.chars().take(1500).collect::<String>()
            );
            if status.is_client_error() {
                return Err(Rejected(message).into());
            }
            bail!(message);
        }
        Ok(response)
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let result = async {
            Self::checked(
                self.request(Method::GET, path)
                    .timeout(REQUEST_TIMEOUT)
                    .send()
                    .await?,
            )
            .await?
            .json()
            .await
            .context("Invalid OpenAI Agents API response")
        }
        .await;
        result.map_err(|e: anyhow::Error| {
            RecoveryNeeded(format!(
                "OpenAI state could not be retrieved; keep this submission for recovery: {e}"
            ))
            .into()
        })
    }

    fn events(response: Response) -> Events {
        sse_stream::SseStream::from_byte_stream(response.bytes_stream())
            .filter_map(|event| async move {
                match event {
                    Ok(event) => event
                        .data
                        .filter(|s| s != "[DONE]")
                        .map(|s| serde_json::from_str(&s).map_err(Into::into)),
                    Err(e) => Some(Err(anyhow!("OpenAI event stream disconnected: {e}"))),
                }
            })
            .boxed()
    }

    async fn subscribe(&self, id: &str) -> Result<Events> {
        // Only the handshake is bounded. A live turn may take longer than an HTTP request.
        let result = async {
            let response = tokio::time::timeout(
                REQUEST_TIMEOUT,
                self.request(Method::GET, &format!("/{id}/events?stream=true"))
                    .header("Accept", "text/event-stream")
                    .send(),
            )
            .await??;
            Ok(Self::events(Self::checked(response).await?))
        }
        .await;
        result.map_err(|e: anyhow::Error| {
            RecoveryNeeded(format!(
                "Could not subscribe to OpenAI session {id}; retry this Talon submission: {e}"
            ))
            .into()
        })
    }

    async fn items(&self, id: &str, after: Option<&str>) -> Result<Vec<Value>> {
        let mut path = format!("/{id}/items?order=asc&limit=100");
        if let Some(after) = after {
            path.push_str(&format!("&after={}", urlencoding::encode(after)));
        }
        let mut items = Vec::new();
        loop {
            let page = self.get(&path).await?;
            items.extend(
                page["data"]
                    .as_array()
                    .context("OpenAI items response has no data")?
                    .iter()
                    .cloned(),
            );
            if page["has_more"] != true {
                break;
            }
            let last = page["last_id"]
                .as_str()
                .context("OpenAI items page has no cursor")?;
            let next = format!(
                "/{id}/items?order=asc&limit=100&after={}",
                urlencoding::encode(last)
            );
            if next == path {
                bail!("OpenAI items pagination did not advance");
            }
            path = next;
        }
        Ok(items)
    }

    async fn find_created_session(&self, nonce: &str) -> Result<Option<String>> {
        // ponytail: creation recovery scans session metadata; replace with a
        // provider lookup by creation key if the API adds one.
        let mut path = "?limit=100&order=desc".to_string();
        loop {
            let page = self.get(&path).await?;
            for session in page["data"]
                .as_array()
                .context("OpenAI sessions response has no data")?
            {
                if session["metadata"]["talon_creation"] == nonce {
                    return Ok(Some(
                        session["id"]
                            .as_str()
                            .context("OpenAI session has no id")?
                            .into(),
                    ));
                }
            }
            if page["has_more"] != true {
                return Ok(None);
            }
            let last = page["last_id"]
                .as_str()
                .context("OpenAI sessions page has no cursor")?;
            let next = format!("?limit=100&order=desc&after={}", urlencoding::encode(last));
            if next == path {
                bail!("OpenAI sessions pagination did not advance");
            }
            path = next;
        }
    }

    async fn save_acceptance(
        &self,
        raw: &[u8],
        state: &RemoteSession,
        input: &AcceptedInput,
    ) -> Result<()> {
        let pending = state
            .pending
            .as_ref()
            .context("Missing pending OpenAI input")?;
        let key = self.key("OpenAIInput", &pending.submission_id);
        let encoded = serde_json::to_vec(input)?;
        if !self.cp.kv.compare_and_swap(&key, None, &encoded).await? {
            let saved: AcceptedInput = self
                .read("OpenAIInput", &pending.submission_id)
                .await?
                .context("Missing OpenAI input receipt")?;
            if saved.index != input.index
                || saved.text != input.text
                || saved.session_id != input.session_id
            {
                bail!("Conflicting OpenAI input receipt; refusing to submit again");
            }
        }
        let mut updated = state.clone();
        updated.session_id = Some(input.session_id.clone());
        updated.pending = None;
        updated.next_input = input.index + 1;
        if !self
            .cp
            .kv
            .compare_and_swap(
                &self.key("OpenAIRuntime", "session"),
                Some(raw),
                &serde_json::to_vec(&updated)?,
            )
            .await?
        {
            bail!("OpenAI session changed while saving acceptance; retry this Talon submission");
        }
        Ok(())
    }

    async fn saved_input_position(&self, index: usize) -> Result<usize> {
        let prefix = keys::session(&self.ns, &self.agent, &self.session)
            .as_parent()
            .list(Some("OpenAIInput"));
        let mut discarded = 0;
        for (_, bytes) in self.cp.kv.list_entries(&prefix, None).await? {
            let input: AcceptedInput = serde_json::from_slice(&bytes)?;
            if input.index < index && input.discarded {
                discarded += 1;
            }
        }
        index
            .checked_sub(discarded)
            .context("Invalid OpenAI input order")
    }

    async fn recover_pending(&self, raw: &[u8], state: &RemoteSession) -> Result<AcceptedInput> {
        let pending = state
            .pending
            .as_ref()
            .context("Missing pending OpenAI input")?;
        let deadline = tokio::time::Instant::now() + RECOVERY_TIMEOUT;
        let mut remote_id = state.session_id.clone();
        loop {
            if remote_id.is_none() {
                remote_id = self.find_created_session(&state.nonce).await?;
            }
            if let Some(id) = &remote_id {
                let items = self.items(id, None).await?;
                if let Some(item) = items
                    .iter()
                    .filter(|i| i["type"] == "message" && i["role"] == "user")
                    .nth(self.saved_input_position(state.next_input).await?)
                {
                    if item_text(item) != pending.text {
                        return Err(RecoveryNeeded("OpenAI input history does not match Talon's submission order; inspect the saved session before retrying".into()).into());
                    }
                    let input = AcceptedInput {
                        session_id: id.clone(),
                        item_id: required_str(item, "id")?,
                        turn_id: required_str(item, "turn_id")?,
                        index: state.next_input,
                        text: pending.text.clone(),
                        cancel_requested: false,
                        observed_turn_id: None,
                        discarded: false,
                    };
                    self.save_acceptance(raw, state, &input).await.map_err(|e|
                        RecoveryNeeded(format!("OpenAI accepted the message, but Talon could not save its receipt; retry this submission: {e}")))?;
                    return Ok(input);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(RecoveryNeeded(format!("OpenAI submission '{}' has an unknown outcome. Inspect remote session {:?} (creation marker {}) and retry the same Talon submission after reconciling; no second request was sent. Clear the Talon session only to explicitly start a new conversation.", pending.submission_id, remote_id, state.nonce)).into());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn submit(
        &self,
        submission: &str,
        attempt: &str,
        text: &str,
    ) -> Result<(AcceptedInput, Events)> {
        loop {
            let key = self.key("OpenAIRuntime", "session");
            let raw = self.cp.kv.get(&key).await?;
            let mut state = match raw.as_deref() {
                Some(bytes) => serde_json::from_slice::<RemoteSession>(bytes)?,
                None => RemoteSession {
                    nonce: crate::control::uuid::session_submission_id(),
                    configuration: self.configuration.clone(),
                    session_id: None,
                    pending: None,
                    next_input: 0,
                },
            };
            if state.configuration != self.configuration {
                bail!("OpenAI session model, provider or instructions changed; clear the Talon session or start a new session to apply the new configuration");
            }
            if let Some(pending) = &state.pending {
                // Recover the crash between saving HTTP acceptance and releasing
                // the short submission reservation.
                if let Some(input) = self
                    .read::<AcceptedInput>("OpenAIInput", &pending.submission_id)
                    .await?
                {
                    self.save_acceptance(
                        raw.as_deref().context("Missing OpenAI state")?,
                        &state,
                        &input,
                    )
                    .await?;
                    continue;
                }
                if pending.submission_id != submission && self.current_attempt(pending).await? {
                    // Only serialize HTTP acceptance, never the remote agent's execution.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
                self.recover_pending(raw.as_deref().context("Missing OpenAI state")?, &state)
                    .await?;
                continue;
            }
            if let Some(input) = self
                .read::<AcceptedInput>("OpenAIInput", submission)
                .await?
            {
                let stream = self.subscribe(&input.session_id).await?;
                return Ok((input, stream));
            }
            // A subscription is read-only. Establish it before reserving input,
            // so a failed handshake cannot leave an uncertain unsent message.
            let subscribed = match &state.session_id {
                Some(id) => Some(self.subscribe(id).await?),
                None => None,
            };
            state.pending = Some(PendingInput {
                submission_id: submission.into(),
                attempt_id: attempt.into(),
                text: text.into(),
            });
            if !self
                .current_attempt(state.pending.as_ref().unwrap())
                .await?
            {
                bail!("OpenAI submission lease is no longer current");
            }
            let prepared = serde_json::to_vec(&state)?;
            if !self
                .cp
                .kv
                .compare_and_swap(&key, raw.as_deref(), &prepared)
                .await?
            {
                continue;
            }

            let sent = async {
                if let Some(id) = &state.session_id {
                    let stream = subscribed.context("Missing OpenAI event subscription")?;
                    Self::checked(self.request(Method::POST, &format!("/{id}/events"))
                        .header("Idempotency-Key", format!("{id}-{submission}"))
                        .timeout(REQUEST_TIMEOUT)
                        .json(&json!({"events":[{"type":"agent.session.input.message","input":[{"role":"user","content":[{"type":"input_text","text":text}]}]}]}))
                        .send().await?).await?;
                    Ok::<_, anyhow::Error>(stream)
                } else {
                    let response = tokio::time::timeout(REQUEST_TIMEOUT,
                        self.request(Method::POST, "").json(&json!({"agent":self.agent_config,
                            "environment":{"type":"none"},"input":text,"stream":true,
                            "metadata":{"talon_creation":state.nonce}})).send()).await??;
                    let mut stream = Self::events(Self::checked(response).await?);
                    loop {
                        let event = tokio::time::timeout(REQUEST_TIMEOUT, stream.next()).await?
                            .context("OpenAI create stream closed before the session id arrived")??;
                        if event["type"] == "agent.session.created" {
                            state.session_id = Some(required_str(&event["session"], "id")?);
                            break;
                        }
                        if event["type"] == "error" { bail!("OpenAI session creation failed: {}", event["error"]); }
                    }
                    Ok(stream)
                }
            }.await;
            if sent
                .as_ref()
                .err()
                .is_some_and(|e| e.downcast_ref::<Rejected>().is_some())
            {
                state.pending = None;
                self.cp
                    .kv
                    .compare_and_swap(&key, Some(&prepared), &serde_json::to_vec(&state)?)
                    .await?;
                return Err(sent.err().unwrap());
            }
            // The prepared marker was durable before the HTTP call. Even when
            // the response is lost, reconcile saved input instead of guessing.
            #[cfg(test)]
            test_pause("accepted").await;
            let input = if sent.is_ok() {
                let input = AcceptedInput {
                    session_id: state
                        .session_id
                        .clone()
                        .context("OpenAI did not return a session id")?,
                    index: state.next_input,
                    text: text.into(),
                    item_id: String::new(),
                    turn_id: String::new(),
                    cancel_requested: false,
                    observed_turn_id: None,
                    discarded: false,
                };
                self.save_acceptance(&prepared, &state, &input).await.map_err(|e|
                    RecoveryNeeded(format!("OpenAI accepted the message, but Talon could not save its receipt; retry this submission: {e}")))?;
                input
            } else {
                self.recover_pending(&prepared, &state)
                    .await
                    .with_context(|| sent.as_ref().err().unwrap().to_string())?
            };
            let stream = match sent {
                Ok(stream) => stream,
                Err(_) => self.subscribe(&input.session_id).await?,
            };
            return Ok((input, stream));
        }
    }

    pub async fn execute(
        &self,
        submission: &str,
        attempt: &str,
        text: &str,
        cancel: &CancellationToken,
    ) -> Result<TurnResult> {
        self.observe(submission, attempt, text, cancel).await.map_err(|error| {
            if error.downcast_ref::<Rejected>().is_some() || error.downcast_ref::<RecoveryNeeded>().is_some() {
                error
            } else {
                RecoveryNeeded(format!("OpenAI execution needs recovery; retry the same Talon submission without sending a new message: {error:#}")).into()
            }
        })
    }

    async fn observe(
        &self,
        submission: &str,
        attempt: &str,
        text: &str,
        cancel: &CancellationToken,
    ) -> Result<TurnResult> {
        let (mut input, mut stream) = self.submit(submission, attempt, text).await?;
        #[cfg(test)]
        if std::env::var("TALON_OPENAI_TEST_OBSERVER_EXIT").is_ok() {
            return Err(RecoveryNeeded(
                "Live test dropped the observer after HTTP acceptance".into(),
            )
            .into());
        }
        #[cfg(test)]
        if std::env::var("TALON_OPENAI_TEST_DISCONNECT").is_ok() {
            stream = futures::stream::empty().boxed();
        }
        let mut disconnected_since = None;
        let mut cancel_sent_turn = None;
        let awaiting_since = tokio::time::Instant::now();
        loop {
            #[cfg(test)]
            test_pause("before_snapshot").await;
            input.cancel_requested |= self
                .read::<bool>("OpenAICancel", submission)
                .await?
                .unwrap_or(false);
            input.cancel_requested |= cancel.is_cancelled();
            let pending = PendingInput {
                submission_id: submission.into(),
                attempt_id: attempt.into(),
                text: String::new(),
            };
            if !self.current_attempt(&pending).await? {
                return Err(RecoveryNeeded("OpenAI observer lost its Talon submission lease; the next attempt must retrieve the saved turn".into()).into());
            }
            let mut active_turn_id = None;
            let turn = if input.turn_id.is_empty() {
                // HTTP 202 can precede the saved input by an entire model step.
                // Other inputs are already free to reach OpenAI while we wait.
                // ponytail: history scan until attribution; add a saved cursor if
                // long conversations make this read materially expensive.
                let items = self.items(&input.session_id, None).await?;
                if let Some(item) = items
                    .iter()
                    .filter(|i| i["type"] == "message" && i["role"] == "user")
                    .nth(self.saved_input_position(input.index).await?)
                {
                    if item_text(item) != input.text {
                        return Err(RecoveryNeeded("OpenAI input history differs from the accepted Talon message; inspect this session before retrying".into()).into());
                    }
                    input.item_id = required_str(item, "id")?;
                    input.turn_id = required_str(item, "turn_id")?;
                    self.cp
                        .kv
                        .set(
                            &self.key("OpenAIInput", submission),
                            &serde_json::to_vec(&input)?,
                        )
                        .await?;
                    self.get(&format!("/{}/turns/{}", input.session_id, input.turn_id))
                        .await?
                } else {
                    let turns = self
                        .get(&format!("/{}/turns?order=desc&limit=1", input.session_id))
                        .await?;
                    let latest = turns["data"].as_array().and_then(|data| data.first());
                    if let Some(latest) = latest {
                        let id = required_str(latest, "id")?;
                        match latest["status"].as_str() {
                            Some("queued" | "in_progress") => {
                                if latest["status"] == "in_progress" {
                                    active_turn_id = Some(id.clone());
                                }
                                if input.observed_turn_id.as_deref() != Some(&id) {
                                    input.observed_turn_id = Some(id);
                                    self.cp
                                        .kv
                                        .set(
                                            &self.key("OpenAIInput", submission),
                                            &serde_json::to_vec(&input)?,
                                        )
                                        .await?;
                                }
                            }
                            Some("cancelled" | "failed")
                                if input.observed_turn_id.as_deref() == Some(&id) =>
                            {
                                // OpenAI discards unapplied steering input when
                                // the active turn is cancelled. Account for the
                                // missing history item before accepting replies
                                // to subsequent messages.
                                input.discarded = true;
                                input.turn_id = id;
                                self.cp
                                    .kv
                                    .set(
                                        &self.key("OpenAIInput", submission),
                                        &serde_json::to_vec(&input)?,
                                    )
                                    .await?;
                            }
                            Some("completed" | "cancelled" | "failed")
                                if awaiting_since.elapsed() > RECOVERY_TIMEOUT =>
                            {
                                return Err(RecoveryNeeded("OpenAI ended its turn without saving this accepted input; inspect the session before retrying this submission".into()).into());
                            }
                            _ => {}
                        }
                    }
                    if input.discarded {
                        latest.unwrap().clone()
                    } else {
                        json!({"status":"awaiting_input"})
                    }
                }
            } else {
                self.get(&format!("/{}/turns/{}", input.session_id, input.turn_id))
                    .await?
            };
            let status = required_str(&turn, "status")?;
            if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
                let mut text = String::new();
                if status == "completed" {
                    for item in self.items(&input.session_id, None).await? {
                        if item["turn_id"] == input.turn_id
                            && item["role"] == "assistant"
                            && item["phase"] == "final_answer"
                            && item["status"] == "completed"
                        {
                            // A steering message can supersede an earlier final
                            // answer inside this same turn. Deliver the last one.
                            text = item_text(&item);
                        }
                    }
                    if text.trim().is_empty() {
                        bail!(
                            "OpenAI completed turn {} without a supported final text answer",
                            input.turn_id
                        );
                    }
                }
                let key = self.key("OpenAITurn", &input.turn_id);
                let proposed = TurnReply {
                    submission_id: submission.into(),
                    message_id: crate::control::uuid::session_message_id(),
                };
                self.cp
                    .kv
                    .compare_and_swap(&key, None, &serde_json::to_vec(&proposed)?)
                    .await?;
                let reply = self
                    .read("OpenAITurn", &input.turn_id)
                    .await?
                    .context("Missing OpenAI turn reply")?;
                #[cfg(test)]
                test_pause("completed").await;
                return Ok(TurnResult {
                    status,
                    text,
                    error: turn
                        .get("error")
                        .filter(|e| !e.is_null())
                        .map(ToString::to_string),
                    reply,
                });
            }
            if status == "in_progress" {
                active_turn_id = Some(required_str(&turn, "id")?);
            }
            // Cancellation before a turn starts can succeed without doing
            // anything. Keep Stop pending until there is an active target.
            if input.cancel_requested
                && active_turn_id.is_some()
                && active_turn_id != cancel_sent_turn
            {
                self.cp
                    .kv
                    .set(
                        &self.key("OpenAIInput", submission),
                        &serde_json::to_vec(&input)?,
                    )
                    .await?;
                let cancelled = async {
                    Self::checked(
                        self.request(Method::POST, &format!("/{}/events", input.session_id))
                            .header(
                                "Idempotency-Key",
                                format!(
                                    "cancel-{}-{submission}-{}",
                                    input.session_id,
                                    active_turn_id.as_deref().unwrap()
                                ),
                            )
                            .timeout(REQUEST_TIMEOUT)
                            .json(&json!({"events":[{"type":"agent.session.input.cancel"}]}))
                            .send()
                            .await?,
                    )
                    .await
                }
                .await;
                cancelled.map_err(|e| {
                    RecoveryNeeded(format!(
                        "OpenAI cancellation needs recovery; the stop request is saved: {e}"
                    ))
                })?;
                cancel_sent_turn = active_turn_id;
            }
            let session = self.get(&format!("/{}", input.session_id)).await?;
            if session["status"] == "requires_action" {
                return Err(Rejected(format!(
                    "OpenAI requires an unsupported action: {}",
                    session["required_actions"]
                ))
                .into());
            }
            if session["status"] == "failed" {
                return Err(
                    Rejected(format!("OpenAI session failed: {}", session["error"])).into(),
                );
            }
            let snapshot_at = tokio::time::Instant::now() + Duration::from_secs(2);
            loop {
                tokio::select! {
                    _ = cancel.cancelled(), if !input.cancel_requested => break,
                    _ = tokio::time::sleep_until(snapshot_at) => break,
                    event = stream.next() => {
                        match event {
                            Some(Ok(event)) => {
                                disconnected_since = None;
                                if event["type"] == "error" { bail!("OpenAI stream error: {}", event["error"]); }
                                if event["turn"]["id"] == input.turn_id && event["turn"]["subagent_id"].is_null()
                                    && matches!(event["type"].as_str(), Some("agent.session.turn.completed" | "agent.session.turn.failed" | "agent.session.turn.cancelled")) { break; }
                            }
                            _ => {
                                let since = *disconnected_since.get_or_insert_with(tokio::time::Instant::now);
                                if since.elapsed() > RECOVERY_TIMEOUT { return Err(RecoveryNeeded(format!("OpenAI stream remains disconnected; turn {} may still be running. Retry this Talon submission to retrieve its saved outcome.",input.turn_id)).into()); }
                                // Subscribe before the next snapshot; saved items,
                                // rather than replayed deltas, are the output authority.
                                match self.subscribe(&input.session_id).await {
                                    Ok(next) => stream = next,
                                    Err(_) => tokio::time::sleep(Duration::from_secs(1)).await,
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn required_str(value: &Value, field: &str) -> Result<String> {
    value[field]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .with_context(|| format!("OpenAI response missing {field}"))
}

fn item_text(item: &Value) -> String {
    item["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["type"] == "input_text" || p["type"] == "output_text")
        .filter_map(|p| p["text"].as_str())
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
pub(crate) async fn test_pause(point: &str) {
    let paused = || {
        std::env::var("TALON_OPENAI_TEST_PAUSE")
            .unwrap_or_default()
            .split(',')
            .any(|name| name == point)
    };
    if paused() {
        std::env::set_var("TALON_OPENAI_TEST_REACHED", point);
        while paused() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

#[cfg(test)]
mod tests;
