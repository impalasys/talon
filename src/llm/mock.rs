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
