// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use async_trait::async_trait;
use prost::Message;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::control::events::{SessionStepEvent, StepType};
use crate::control::{topics, KeyValueStore, MessagePublisher};
use crate::core::context_budget::tool_result_preview;
use crate::core::executor::{AgentEvent, ExecutionSink};
use crate::gateway::rpc::models;
use crate::llm::ChatUsage;

#[derive(Debug, Clone)]
pub struct SessionRunSummary {
    pub duration_ms: u128,
    pub input_token_chunks: u64,
    pub input_token_chars: usize,
    pub published_token_batches: u64,
    pub published_token_chars: usize,
    pub reasoning_chunks: u64,
    pub reasoning_chars: usize,
    pub tool_calls: u64,
    pub tool_results: u64,
    pub usage_events: u64,
}

/// Production sink: accumulates tokens, throttle-flushes partial text to KV,
/// and publishes `SessionStepEvent`s to PubSub for real-time UI streaming.
pub struct PubSubSessionSink {
    pub kv: Arc<dyn KeyValueStore>,
    pub pubsub: Arc<dyn MessagePublisher>,
    pub ns: String,
    pub session_id: String,
    pub agent_id: String,
    pub reply_msg_id: String,
    pub reply_msg_key: String,
    pub status_topic: String,
    token_publish_interval: Duration,
    started_at: Instant,
    accumulated: Mutex<String>,
    persisted_text_buffer: Mutex<String>,
    pending_token_event_buffer: Mutex<String>,
    next_step_index: Mutex<u64>,
    last_flush: Mutex<Instant>,
    last_token_publish: Mutex<Instant>,
    input_token_chunks: Mutex<u64>,
    input_token_chars: Mutex<usize>,
    published_token_batches: Mutex<u64>,
    published_token_chars: Mutex<usize>,
    reasoning_chunks: Mutex<u64>,
    reasoning_chars: Mutex<usize>,
    tool_calls: Mutex<u64>,
    tool_results: Mutex<u64>,
    usage_events: Mutex<u64>,
}

impl PubSubSessionSink {
    pub fn new(
        kv: Arc<dyn KeyValueStore>,
        pubsub: Arc<dyn MessagePublisher>,
        ns: impl Into<String>,
        session_id: impl Into<String>,
        agent_id: impl Into<String>,
        reply_msg_id: impl Into<String>,
        reply_msg_key: impl Into<String>,
    ) -> Self {
        Self::new_with_token_publish_interval(
            kv,
            pubsub,
            ns,
            session_id,
            agent_id,
            reply_msg_id,
            reply_msg_key,
            token_publish_interval(),
        )
    }

    fn new_with_token_publish_interval(
        kv: Arc<dyn KeyValueStore>,
        pubsub: Arc<dyn MessagePublisher>,
        ns: impl Into<String>,
        session_id: impl Into<String>,
        agent_id: impl Into<String>,
        reply_msg_id: impl Into<String>,
        reply_msg_key: impl Into<String>,
        token_publish_interval: Duration,
    ) -> Self {
        let session_id = session_id.into();
        let status_topic = topics::session_step_topic_for_session(&session_id);
        Self {
            kv,
            pubsub,
            ns: ns.into(),
            session_id,
            agent_id: agent_id.into(),
            reply_msg_id: reply_msg_id.into(),
            reply_msg_key: reply_msg_key.into(),
            status_topic,
            token_publish_interval,
            started_at: Instant::now(),
            accumulated: Mutex::new(String::new()),
            persisted_text_buffer: Mutex::new(String::new()),
            pending_token_event_buffer: Mutex::new(String::new()),
            next_step_index: Mutex::new(0),
            last_flush: Mutex::new(Instant::now()),
            last_token_publish: Mutex::new(Instant::now()),
            input_token_chunks: Mutex::new(0),
            input_token_chars: Mutex::new(0),
            published_token_batches: Mutex::new(0),
            published_token_chars: Mutex::new(0),
            reasoning_chunks: Mutex::new(0),
            reasoning_chars: Mutex::new(0),
            tool_calls: Mutex::new(0),
            tool_results: Mutex::new(0),
            usage_events: Mutex::new(0),
        }
    }

    fn next_step_id(&self) -> String {
        let mut next = self.next_step_index.lock().unwrap();
        *next += 1;
        format!("{:06}", *next)
    }

    async fn persist_step(
        &self,
        step_type: StepType,
        name: String,
        content: String,
        payload_json: String,
    ) {
        let key = crate::control::keys::session_message_step(
            &self.agent_id,
            &self.session_id,
            &self.reply_msg_id,
            &self.next_step_id(),
        );
        let event = SessionStepEvent {
            session_id: self.session_id.clone(),
            step_type: step_type as i32,
            content,
            timestamp: chrono::Utc::now().timestamp_micros(),
            agent: self.agent_id.clone(),
            ns: self.ns.clone(),
            message_id: self.reply_msg_id.clone(),
            name,
            payload_json,
        };
        let _ = crate::control::ProtoKeyValueStoreExt::set_msg(
            self.kv.as_ref(),
            &self.ns,
            &key,
            &event,
        )
        .await;
    }

    async fn flush_persisted_text(&self) {
        let content = {
            let mut buffer = self.persisted_text_buffer.lock().unwrap();
            if buffer.is_empty() {
                return;
            }
            std::mem::take(&mut *buffer)
        };

        self.persist_step(StepType::Token, String::new(), content, String::new())
            .await;
    }

    async fn flush_token_event_buffer(&self) {
        let content = {
            let mut buffer = self.pending_token_event_buffer.lock().unwrap();
            if buffer.is_empty() {
                return;
            }
            std::mem::take(&mut *buffer)
        };

        *self.last_token_publish.lock().unwrap() = Instant::now();
        *self.published_token_batches.lock().unwrap() += 1;
        *self.published_token_chars.lock().unwrap() += content.len();
        self.publish_event(AgentEvent::Token(content)).await;
    }

    async fn publish_event(&self, event: AgentEvent) {
        let (step_type, name, content, payload_json) = match event {
            AgentEvent::Reasoning(content) => {
                (StepType::Reasoning, String::new(), content, String::new())
            }
            AgentEvent::Action { id, name, input } => (
                StepType::Action,
                name,
                "Tool call".to_string(),
                serde_json::to_string(&serde_json::json!({
                    "tool_call_id": id,
                    "input": input,
                }))
                .unwrap_or_else(|_| "{}".to_string()),
            ),
            AgentEvent::Observation { id, name, output } => (
                StepType::Observation,
                name,
                output.clone(),
                serde_json::to_string(&serde_json::json!({
                    "tool_call_id": id,
                    "output": output,
                }))
                .unwrap_or_else(|_| "{}".to_string()),
            ),
            AgentEvent::Token(content) => (StepType::Token, String::new(), content, String::new()),
            AgentEvent::Usage(usage) => (
                StepType::Usage,
                String::new(),
                String::new(),
                serde_json::to_string(&usage).unwrap_or_else(|_| "{}".to_string()),
            ),
            AgentEvent::Done(reply) => (StepType::Done, String::new(), reply, String::new()),
            AgentEvent::Error(err) => (StepType::Error, String::new(), err, String::new()),
        };

        let event = SessionStepEvent {
            session_id: self.session_id.clone(),
            step_type: step_type as i32,
            content,
            timestamp: chrono::Utc::now().timestamp_micros(),
            agent: self.agent_id.clone(),
            ns: self.ns.clone(),
            message_id: self.reply_msg_id.clone(),
            name,
            payload_json,
        };
        let _ = self
            .pubsub
            .publish(&self.status_topic, &event.encode_to_vec())
            .await;
    }

    fn maybe_flush_kv(&self, current_text: String) {
        let should_flush = {
            let last = self.last_flush.lock().unwrap();
            last.elapsed().as_millis() > 1000
        };
        if should_flush {
            *self.last_flush.lock().unwrap() = Instant::now();
            let kv = self.kv.clone();
            let ns = self.ns.clone();
            let key = self.reply_msg_key.clone();
            let msg_id = self.reply_msg_id.clone();
            tokio::spawn(async move {
                let partial = models::SessionMessage {
                    id: msg_id,
                    role: 2, // ASSISTANT
                    content: current_text,
                    created_at: chrono::Utc::now().timestamp_micros(),
                    labels: std::collections::HashMap::new(),
                };
                if let Err(e) =
                    crate::control::ProtoKeyValueStoreExt::set_msg(kv.as_ref(), &ns, &key, &partial)
                        .await
                {
                    tracing::error!("Failed to persist partial message: {}", e);
                }
            });
        }
    }

    fn should_flush_token_event(&self) -> bool {
        let last = self.last_token_publish.lock().unwrap();
        last.elapsed() >= self.token_publish_interval
    }

    pub fn summary(&self) -> SessionRunSummary {
        SessionRunSummary {
            duration_ms: self.started_at.elapsed().as_millis(),
            input_token_chunks: *self.input_token_chunks.lock().unwrap(),
            input_token_chars: *self.input_token_chars.lock().unwrap(),
            published_token_batches: *self.published_token_batches.lock().unwrap(),
            published_token_chars: *self.published_token_chars.lock().unwrap(),
            reasoning_chunks: *self.reasoning_chunks.lock().unwrap(),
            reasoning_chars: *self.reasoning_chars.lock().unwrap(),
            tool_calls: *self.tool_calls.lock().unwrap(),
            tool_results: *self.tool_results.lock().unwrap(),
            usage_events: *self.usage_events.lock().unwrap(),
        }
    }
}

#[async_trait]
impl ExecutionSink for PubSubSessionSink {
    async fn on_token(&self, token: &str) {
        *self.input_token_chunks.lock().unwrap() += 1;
        *self.input_token_chars.lock().unwrap() += token.len();
        let current_text = {
            let mut acc = self.accumulated.lock().unwrap();
            acc.push_str(token);
            acc.clone()
        };
        self.persisted_text_buffer.lock().unwrap().push_str(token);
        self.pending_token_event_buffer
            .lock()
            .unwrap()
            .push_str(token);
        self.maybe_flush_kv(current_text);
        if self.should_flush_token_event() {
            self.flush_token_event_buffer().await;
        }
    }

    async fn on_tool_call(&self, id: &str, name: &str, input: &Value) {
        *self.tool_calls.lock().unwrap() += 1;
        self.flush_persisted_text().await;
        self.flush_token_event_buffer().await;
        self.persist_step(
            StepType::Action,
            name.to_string(),
            "Tool call".to_string(),
            serde_json::to_string(&serde_json::json!({
                "tool_call_id": id,
                "input": input,
            }))
            .unwrap_or_else(|_| "{}".to_string()),
        )
        .await;
        self.publish_event(AgentEvent::Action {
            id: id.to_string(),
            name: name.to_string(),
            input: input.clone(),
        })
        .await;
    }

    async fn on_reasoning(&self, reasoning: &str) {
        *self.reasoning_chunks.lock().unwrap() += 1;
        *self.reasoning_chars.lock().unwrap() += reasoning.len();
        self.persist_step(
            StepType::Reasoning,
            String::new(),
            reasoning.to_string(),
            String::new(),
        )
        .await;
        self.publish_event(AgentEvent::Reasoning(reasoning.to_string()))
            .await;
    }

    async fn on_tool_result(&self, id: &str, name: &str, result: &str) {
        *self.tool_results.lock().unwrap() += 1;
        let preview = tool_result_preview(result);
        self.persist_step(
            StepType::Observation,
            name.to_string(),
            preview.clone(),
            serde_json::to_string(&serde_json::json!({
                "tool_call_id": id,
                "output_preview": preview,
                "output": result,
            }))
            .unwrap_or_else(|_| "{}".to_string()),
        )
        .await;
        self.publish_event(AgentEvent::Observation {
            id: id.to_string(),
            name: name.to_string(),
            output: result.to_string(),
        })
        .await;
    }

    async fn on_usage(&self, usage: &ChatUsage) {
        *self.usage_events.lock().unwrap() += 1;
        self.persist_step(
            StepType::Usage,
            String::new(),
            String::new(),
            serde_json::to_string(usage).unwrap_or_else(|_| "{}".to_string()),
        )
        .await;
        self.publish_event(AgentEvent::Usage(usage.clone())).await;
    }

    async fn on_done(&self, reply: &str) {
        self.flush_persisted_text().await;
        self.flush_token_event_buffer().await;
        // Final KV write (complete message)
        let kv = self.kv.clone();
        let ns = self.ns.clone();
        let key = self.reply_msg_key.clone();
        let msg_id = self.reply_msg_id.clone();
        let reply = reply.to_string();
        let reply_for_event = reply.clone();
        tokio::spawn(async move {
            let msg = models::SessionMessage {
                id: msg_id,
                role: 2,
                content: reply,
                created_at: chrono::Utc::now().timestamp_micros(),
                labels: std::collections::HashMap::new(),
            };
            let _ =
                crate::control::ProtoKeyValueStoreExt::set_msg(kv.as_ref(), &ns, &key, &msg).await;
        });

        self.publish_event(AgentEvent::Done(reply_for_event)).await;
    }

    async fn on_error(&self, err: &str) {
        self.flush_persisted_text().await;
        self.flush_token_event_buffer().await;
        self.persist_step(
            StepType::Error,
            String::new(),
            err.to_string(),
            String::new(),
        )
        .await;
        self.publish_event(AgentEvent::Error(err.to_string())).await;
    }
}

fn token_publish_interval() -> Duration {
    std::env::var("TALON_TOKEN_BATCH_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(250))
}

#[cfg(test)]
mod tests {
    use super::{token_publish_interval, PubSubSessionSink};
    use crate::control::events::{SessionStepEvent, StepType};
    use crate::control::{KeyValueStore, MessagePublisher};
    use crate::core::executor::ExecutionSink;
    use async_trait::async_trait;
    use prost::Message;
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockKvStore {
        entries: Arc<Mutex<Vec<(String, String, Vec<u8>)>>>,
    }

    #[async_trait]
    impl KeyValueStore for MockKvStore {
        async fn get(&self, _ns: &str, _k: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(None)
        }
        async fn set(&self, ns: &str, key: &str, value: &[u8]) -> anyhow::Result<()> {
            self.entries
                .lock()
                .await
                .push((ns.to_string(), key.to_string(), value.to_vec()));
            Ok(())
        }
        async fn compare_and_swap(
            &self,
            _ns: &str,
            _k: &str,
            _expected: Option<&[u8]>,
            _value: &[u8],
        ) -> anyhow::Result<bool> {
            Ok(true)
        }
        async fn delete(&self, _ns: &str, _k: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn list_keys(&self, _ns: &str, _p: &str) -> anyhow::Result<Vec<String>> {
            Ok(vec![])
        }
    }

    struct MockPubSub {
        events: Arc<Mutex<Vec<SessionStepEvent>>>,
    }

    #[async_trait]
    impl MessagePublisher for MockPubSub {
        async fn publish(&self, _topic: &str, message: &[u8]) -> anyhow::Result<()> {
            let event = SessionStepEvent::decode(message)?;
            self.events.lock().await.push(event);
            Ok(())
        }

        async fn subscribe(
            &self,
            _topic: &str,
        ) -> anyhow::Result<std::pin::Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>>
        {
            Ok(Box::pin(futures::stream::empty()))
        }
    }

    #[tokio::test]
    async fn token_events_are_batched_by_time_window() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv,
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            "reply-key",
            Duration::from_millis(5),
        );

        sink.on_token("hello").await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        sink.on_token(" world").await;
        sink.on_done("hello world").await;

        let events = events.lock().await.clone();
        let token_events = events
            .iter()
            .filter(|event| event.step_type == StepType::Token as i32)
            .map(|event| event.content.clone())
            .collect::<Vec<_>>();

        assert_eq!(token_events, vec!["hello world".to_string()]);
    }

    #[tokio::test]
    async fn token_buffer_flushes_before_tool_call_boundary() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv,
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            "reply-key",
            Duration::from_secs(10),
        );

        sink.on_token("drafting ").await;
        sink.on_token("request").await;
        sink.on_tool_call("tool-1", "create_prompt", &json!({"content": "x"}))
            .await;

        let events = events.lock().await.clone();
        assert_eq!(events[0].step_type, StepType::Token as i32);
        assert_eq!(events[0].content, "drafting request");
        assert_eq!(events[1].step_type, StepType::Action as i32);
        assert_eq!(events[1].name, "create_prompt");
    }

    #[tokio::test]
    async fn tool_results_store_preview_in_content_and_raw_output_in_payload() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            "reply-key",
            Duration::from_secs(10),
        );
        let raw_output = format!(
            "{{\"items\":[{{\"path\":\"footer.tsx\",\"content\":\"{}\"}}]}}",
            "x".repeat(20_000)
        );

        sink.on_tool_result("tool-1", "mcp_github_get_file_contents", &raw_output)
            .await;

        let entries = kv.entries.lock().await.clone();
        let persisted = entries
            .iter()
            .find_map(|(_, _, value)| SessionStepEvent::decode(value.as_slice()).ok())
            .filter(|event| event.step_type == StepType::Observation as i32)
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&persisted.payload_json).unwrap();

        assert!(persisted.content.len() < raw_output.len());
        assert_eq!(payload["output"], raw_output);
        assert_eq!(payload["output_preview"], persisted.content);
    }

    #[tokio::test]
    async fn done_and_error_persist_and_publish_expected_events() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv.clone(),
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            "reply-key",
            Duration::from_secs(10),
        );

        sink.on_token("partial ").await;
        sink.on_error("tool failed").await;
        sink.on_done("final reply").await;
        tokio::time::sleep(Duration::from_millis(25)).await;

        let events = events.lock().await.clone();
        assert!(events.iter().any(
            |event| event.step_type == StepType::Error as i32 && event.content == "tool failed"
        ));
        assert!(events.iter().any(
            |event| event.step_type == StepType::Done as i32 && event.content == "final reply"
        ));

        let entries = kv.entries.lock().await.clone();
        let persisted_messages = entries
            .iter()
            .filter_map(|(_, _, value)| {
                crate::gateway::rpc::models::SessionMessage::decode(value.as_slice()).ok()
            })
            .collect::<Vec<_>>();
        assert!(persisted_messages
            .iter()
            .any(|msg| msg.id == "reply-1" && msg.content == "final reply"));

        let persisted_steps = entries
            .iter()
            .filter_map(|(_, _, value)| SessionStepEvent::decode(value.as_slice()).ok())
            .collect::<Vec<_>>();
        assert!(persisted_steps
            .iter()
            .any(|event| event.step_type == StepType::Token as i32 && event.content == "partial "));
        assert!(persisted_steps.iter().any(
            |event| event.step_type == StepType::Error as i32 && event.content == "tool failed"
        ));
    }

    #[tokio::test]
    async fn summary_tracks_tokens_tool_calls_and_results() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kv = Arc::new(MockKvStore::default());
        let sink = PubSubSessionSink::new_with_token_publish_interval(
            kv,
            Arc::new(MockPubSub {
                events: events.clone(),
            }),
            "conic",
            "session-1",
            "infra",
            "reply-1",
            "reply-key",
            Duration::from_millis(1),
        );

        sink.on_token("hi").await;
        tokio::time::sleep(Duration::from_millis(2)).await;
        sink.on_token(" there").await;
        sink.on_tool_call("tool-1", "search", &json!({"q": "talon"}))
            .await;
        sink.on_tool_result("tool-1", "search", "result body").await;
        sink.on_done("hi there").await;

        let summary = sink.summary();
        assert_eq!(summary.input_token_chunks, 2);
        assert_eq!(summary.input_token_chars, "hi there".len());
        assert!(summary.published_token_batches >= 1);
        assert!(summary.published_token_chars >= "hi there".len());
        assert_eq!(summary.tool_calls, 1);
        assert_eq!(summary.tool_results, 1);
        assert!(summary.duration_ms <= 10_000);
    }

    #[test]
    fn token_publish_interval_uses_env_override_and_defaults() {
        let _guard = crate::test_support::env_lock();
        std::env::remove_var("TALON_TOKEN_BATCH_MS");
        assert_eq!(token_publish_interval(), Duration::from_millis(250));

        std::env::set_var("TALON_TOKEN_BATCH_MS", "5");
        assert_eq!(token_publish_interval(), Duration::from_millis(5));

        std::env::set_var("TALON_TOKEN_BATCH_MS", "0");
        assert_eq!(token_publish_interval(), Duration::from_millis(250));

        std::env::set_var("TALON_TOKEN_BATCH_MS", "not-a-number");
        assert_eq!(token_publish_interval(), Duration::from_millis(250));

        std::env::remove_var("TALON_TOKEN_BATCH_MS");
    }
}
