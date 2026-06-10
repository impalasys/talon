// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::gateway::rpc::manifests;
use crate::memory::Embedding;
use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatContentPart {
    Text {
        text: String,
    },
    ImageUrl {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    ImageData {
        media_type: String,
        data_base64: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

pub fn content_parts_text(parts: &[ChatContentPart]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            ChatContentPart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_parts: Vec<ChatContentPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        let content = content.into();
        Self {
            role: role.into(),
            content_parts: if content.is_empty() {
                Vec::new()
            } else {
                vec![ChatContentPart::Text { text: content }]
            },
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn text_content(&self) -> String {
        content_parts_text(&self.content_parts)
    }

    pub fn is_empty_content(&self) -> bool {
        self.content_parts.iter().all(|part| match part {
            ChatContentPart::Text { text } => text.is_empty(),
            _ => false,
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<ChatUsage>,
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
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<manifests::ThinkingConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct ChatUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum ChatStreamEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallDelta(ToolCallDelta),
    Usage(ChatUsage),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
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
    async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse>;

    async fn stream_chat_completion(&self, request: ChatRequest) -> Result<ChatStream>;
    async fn completion(&self, prompt: &str) -> Result<String>;
}
