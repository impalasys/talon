// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::harness::llm::provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider};
use crate::harness::memory::Embedding;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct UsageTracker {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_cost_usd: f64,
}

pub struct FailoverProvider {
    pub providers: Vec<Arc<dyn LlmProvider>>,
    pub max_retries: usize,
    pub usage: Arc<Mutex<UsageTracker>>,
}

impl FailoverProvider {
    pub fn new(providers: Vec<Arc<dyn LlmProvider>>) -> Self {
        Self {
            providers,
            max_retries: 3,
            usage: Arc::new(Mutex::new(UsageTracker {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_cost_usd: 0.0,
            })),
        }
    }

    async fn with_failover<F, Fut, T>(&self, f: F) -> Result<T>
    where
        F: Fn(Arc<dyn LlmProvider>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut last_error = anyhow!("No providers available");

        for (p_idx, provider) in self.providers.iter().enumerate() {
            let mut delay = 100;
            for retry in 0..self.max_retries {
                match f(provider.clone()).await {
                    Ok(val) => return Ok(val),
                    Err(e) => {
                        last_error = e;
                        eprintln!(
                            "Provider {} failed (attempt {}): {}",
                            p_idx,
                            retry + 1,
                            last_error
                        );
                        if retry < self.max_retries - 1 {
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                            delay *= 2;
                        }
                    }
                }
            }
        }

        Err(last_error)
    }
}

#[async_trait]
impl LlmProvider for FailoverProvider {
    async fn generate_embedding(&self, text: &str) -> Result<Embedding> {
        self.with_failover(|p| {
            let text = text.to_string();
            async move { p.generate_embedding(&text).await }
        })
        .await
    }

    async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse> {
        self.with_failover(|p| {
            let request = request.clone();
            async move { p.chat_completion(request).await }
        })
        .await
    }

    async fn stream_chat_completion(&self, request: ChatRequest) -> Result<ChatStream> {
        self.with_failover(|p| {
            let request = request.clone();
            async move { p.stream_chat_completion(request).await }
        })
        .await
    }

    async fn completion(&self, prompt: &str) -> Result<String> {
        self.with_failover(|p| {
            let prompt = prompt.to_string();
            async move { p.completion(&prompt).await }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::llm::mock::MockLlmProvider;
    use crate::harness::llm::{chat_stream_event, text_delta_event};
    use futures::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FailingMockProvider;
    #[async_trait]
    impl LlmProvider for FailingMockProvider {
        async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
            Err(anyhow!("Always fails"))
        }
        async fn chat_completion(&self, _request: ChatRequest) -> Result<ChatResponse> {
            Err(anyhow!("Always fails"))
        }
        async fn stream_chat_completion(&self, _request: ChatRequest) -> Result<ChatStream> {
            Err(anyhow!("Always fails"))
        }
        async fn completion(&self, _prompt: &str) -> Result<String> {
            Err(anyhow!("Always fails"))
        }
    }

    #[tokio::test]
    async fn test_failover_uses_first_provider() {
        let providers: Vec<Arc<dyn LlmProvider>> =
            vec![Arc::new(MockLlmProvider), Arc::new(FailingMockProvider)];
        let failover = FailoverProvider::new(providers);
        let resp = failover.completion("test").await.unwrap();
        assert!(resp.contains("Mock response"));
    }

    #[tokio::test]
    async fn test_failover_skips_failed_provider() {
        let providers: Vec<Arc<dyn LlmProvider>> =
            vec![Arc::new(FailingMockProvider), Arc::new(MockLlmProvider)];
        let mut failover = FailoverProvider::new(providers);
        failover.max_retries = 1; // Speed up test
        let resp = failover.completion("test").await.unwrap();
        assert!(resp.contains("Mock response"));
    }

    #[tokio::test]
    async fn test_failover_all_fail() {
        let providers: Vec<Arc<dyn LlmProvider>> =
            vec![Arc::new(FailingMockProvider), Arc::new(FailingMockProvider)];
        let mut failover = FailoverProvider::new(providers);
        failover.max_retries = 1;
        let resp = failover.completion("test").await;
        assert!(resp.is_err());
    }

    #[tokio::test]
    async fn test_failover_streaming_fallback() {
        let providers: Vec<Arc<dyn LlmProvider>> =
            vec![Arc::new(FailingMockProvider), Arc::new(MockLlmProvider)];
        let mut failover = FailoverProvider::new(providers);
        failover.max_retries = 1;
        let stream = failover
            .stream_chat_completion(ChatRequest {
                messages: vec![],
                tools: vec![],
                thinking: None,
            })
            .await
            .unwrap();
        let items: Vec<_> = stream.collect().await;
        assert_eq!(items.len(), 1);
        match items[0].as_ref().unwrap() {
            crate::harness::llm::ChatStreamEvent {
                event: Some(chat_stream_event::Event::TextDelta(text)),
            } => assert!(text.contains("Mock response")),
            other => panic!("Unexpected stream event: {:?}", other),
        }
    }

    struct CountingProvider {
        embedding_calls: Arc<AtomicUsize>,
        chat_calls: Arc<AtomicUsize>,
        stream_calls: Arc<AtomicUsize>,
        completion_calls: Arc<AtomicUsize>,
        fail_until: usize,
    }

    #[async_trait]
    impl LlmProvider for CountingProvider {
        async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
            let call = self.embedding_calls.fetch_add(1, Ordering::SeqCst);
            if call < self.fail_until {
                Err(anyhow!("embedding fail {}", call + 1))
            } else {
                Ok(vec![1.0, 2.0, 3.0])
            }
        }

        async fn chat_completion(&self, _request: ChatRequest) -> Result<ChatResponse> {
            let call = self.chat_calls.fetch_add(1, Ordering::SeqCst);
            if call < self.fail_until {
                Err(anyhow!("chat fail {}", call + 1))
            } else {
                Ok(ChatResponse {
                    content: "chat ok".to_string(),
                    tool_calls: vec![],
                    usage: None,
                })
            }
        }

        async fn stream_chat_completion(&self, _request: ChatRequest) -> Result<ChatStream> {
            let call = self.stream_calls.fetch_add(1, Ordering::SeqCst);
            if call < self.fail_until {
                Err(anyhow!("stream fail {}", call + 1))
            } else {
                Ok(Box::pin(futures::stream::iter(vec![Ok(text_delta_event(
                    "stream ok",
                ))])))
            }
        }

        async fn completion(&self, _prompt: &str) -> Result<String> {
            let call = self.completion_calls.fetch_add(1, Ordering::SeqCst);
            if call < self.fail_until {
                Err(anyhow!("completion fail {}", call + 1))
            } else {
                Ok("completion ok".to_string())
            }
        }
    }

    #[tokio::test]
    async fn failover_retries_same_provider_until_success_for_each_entrypoint() {
        let provider = Arc::new(CountingProvider {
            embedding_calls: Arc::new(AtomicUsize::new(0)),
            chat_calls: Arc::new(AtomicUsize::new(0)),
            stream_calls: Arc::new(AtomicUsize::new(0)),
            completion_calls: Arc::new(AtomicUsize::new(0)),
            fail_until: 2,
        });
        let mut failover = FailoverProvider::new(vec![provider.clone()]);
        failover.max_retries = 3;

        let embedding = failover.generate_embedding("hello").await.unwrap();
        assert_eq!(embedding, vec![1.0, 2.0, 3.0]);
        assert_eq!(provider.embedding_calls.load(Ordering::SeqCst), 3);

        let response = failover
            .chat_completion(ChatRequest {
                messages: vec![],
                tools: vec![],
                thinking: None,
            })
            .await
            .unwrap();
        assert_eq!(response.content, "chat ok");
        assert_eq!(provider.chat_calls.load(Ordering::SeqCst), 3);

        let stream = failover
            .stream_chat_completion(ChatRequest {
                messages: vec![],
                tools: vec![],
                thinking: None,
            })
            .await
            .unwrap();
        let items: Vec<_> = stream.collect().await;
        assert_eq!(items.len(), 1);
        assert_eq!(provider.stream_calls.load(Ordering::SeqCst), 3);

        let completion = failover.completion("hello").await.unwrap();
        assert_eq!(completion, "completion ok");
        assert_eq!(provider.completion_calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn failover_returns_last_error_when_retries_exhaust() {
        let provider = Arc::new(CountingProvider {
            embedding_calls: Arc::new(AtomicUsize::new(0)),
            chat_calls: Arc::new(AtomicUsize::new(0)),
            stream_calls: Arc::new(AtomicUsize::new(0)),
            completion_calls: Arc::new(AtomicUsize::new(0)),
            fail_until: usize::MAX,
        });
        let mut failover = FailoverProvider::new(vec![provider.clone()]);
        failover.max_retries = 2;

        let err = failover.generate_embedding("hello").await.unwrap_err();
        assert!(err.to_string().contains("embedding fail 2"));
        assert_eq!(provider.embedding_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failover_handles_empty_provider_list_and_initial_usage_state() {
        let failover = FailoverProvider::new(vec![]);
        let err = failover.completion("hello").await.unwrap_err();
        assert!(err.to_string().contains("No providers available"));

        let usage = failover.usage.lock().unwrap();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_cost_usd, 0.0);
    }
}
