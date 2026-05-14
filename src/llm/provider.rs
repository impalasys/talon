use crate::memory::Embedding;
use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum ChatStreamEvent {
    TextDelta(String),
    ToolCallDelta(ToolCallDelta),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatStreamEvent>> + Send>>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate_embedding(&self, text: &str) -> Result<Embedding>;

    /// Send a chat request with optional tool definitions.
    /// `tools` should be structured as provider-agnostic `Tool` objects.
    /// The provider implementation is responsible for formatting these into
    /// the provider's specific tool/function schema format.
    async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<Tool>,
    ) -> Result<ChatResponse>;

    async fn stream_chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<Tool>,
    ) -> Result<ChatStream>;
    async fn completion(&self, prompt: &str) -> Result<String>;
}
