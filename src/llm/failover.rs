use crate::llm::provider::{ChatMessage, ChatResponse, ChatStream, ChatStreamEvent, LlmProvider};
use crate::memory::Embedding;
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

    async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<crate::llm::provider::Tool>,
    ) -> Result<ChatResponse> {
        self.with_failover(|p| {
            let messages = messages.clone();
            let tools = tools.clone();
            async move { p.chat_completion(messages, tools).await }
        })
        .await
    }

    async fn stream_chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<crate::llm::provider::Tool>,
    ) -> Result<ChatStream> {
        self.with_failover(|p| {
            let messages = messages.clone();
            let tools = tools.clone();
            async move { p.stream_chat_completion(messages, tools).await }
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
    use crate::llm::mock::MockLlmProvider;
    use futures::StreamExt;

    struct FailingMockProvider;
    #[async_trait]
    impl LlmProvider for FailingMockProvider {
        async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
            Err(anyhow!("Always fails"))
        }
        async fn chat_completion(
            &self,
            _messages: Vec<ChatMessage>,
            _tools: Vec<crate::llm::provider::Tool>,
        ) -> Result<ChatResponse> {
            Err(anyhow!("Always fails"))
        }
        async fn stream_chat_completion(
            &self,
            _messages: Vec<ChatMessage>,
            _tools: Vec<crate::llm::provider::Tool>,
        ) -> Result<ChatStream> {
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
            .stream_chat_completion(vec![], vec![])
            .await
            .unwrap();
        let items: Vec<_> = stream.collect().await;
        assert_eq!(items.len(), 1);
        match items[0].as_ref().unwrap() {
            ChatStreamEvent::TextDelta(text) => assert!(text.contains("Mock response")),
            other => panic!("Unexpected stream event: {:?}", other),
        }
    }
}
