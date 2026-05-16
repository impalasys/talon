// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::llm::provider::{ChatMessage, ChatResponse, ChatStream, ChatStreamEvent, LlmProvider};
use crate::memory::Embedding;
use anyhow::Result;
use async_trait::async_trait;

pub struct MockLlmProvider;

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn generate_embedding(&self, _text: &str) -> Result<Embedding> {
        Ok(vec![0.0; 768])
    }

    async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        _tools: Vec<crate::llm::provider::Tool>,
    ) -> Result<ChatResponse> {
        Ok(ChatResponse {
            content: format!("Mock response to {} messages", messages.len()),
            tool_calls: vec![],
        })
    }

    async fn stream_chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        _tools: Vec<crate::llm::provider::Tool>,
    ) -> Result<ChatStream> {
        let response = self.chat_completion(messages, vec![]).await?;
        let stream =
            futures::stream::once(async move { Ok(ChatStreamEvent::TextDelta(response.content)) });
        Ok(Box::pin(stream))
    }

    async fn completion(&self, prompt: &str) -> Result<String> {
        Ok(format!("Mock response for: {}", prompt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn mock_provider_returns_embedding_chat_stream_and_completion() {
        let provider = MockLlmProvider;
        let embedding = provider
            .generate_embedding("hello")
            .await
            .expect("embedding should succeed");
        assert_eq!(embedding.len(), 768);
        assert!(embedding.iter().all(|value| *value == 0.0));

        let response = provider
            .chat_completion(
                vec![ChatMessage {
                    role: "user".to_string(),
                    content: "hi".to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                }],
                vec![],
            )
            .await
            .expect("chat completion should succeed");
        assert_eq!(response.content, "Mock response to 1 messages");
        assert!(response.tool_calls.is_empty());

        let stream = provider
            .stream_chat_completion(
                vec![ChatMessage {
                    role: "user".to_string(),
                    content: "stream".to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                }],
                vec![],
            )
            .await
            .expect("streaming should succeed");
        let events = stream
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<anyhow::Result<Vec<_>>>()
            .expect("stream items should succeed");
        assert_eq!(
            events,
            vec![ChatStreamEvent::TextDelta(
                "Mock response to 1 messages".to_string()
            )]
        );

        let text = provider
            .completion("prompt")
            .await
            .expect("completion should succeed");
        assert_eq!(text, "Mock response for: prompt");
    }
}
