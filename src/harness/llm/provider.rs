// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::harness::memory::Embedding;
use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

pub use crate::gateway::rpc::harness_proto::{
    chat_content_part, chat_stream_event, ChatContentPart, ChatImageData, ChatImageUrl,
    ChatMessage, ChatRequest, ChatResponse, ChatStreamEvent, ChatUsage, Tool, ToolCall,
    ToolCallDelta,
};

pub fn text_part(text: impl Into<String>) -> ChatContentPart {
    ChatContentPart {
        content: Some(chat_content_part::Content::Text(text.into())),
    }
}

pub fn image_url_part(
    url: impl Into<String>,
    detail: Option<impl Into<String>>,
) -> ChatContentPart {
    ChatContentPart {
        content: Some(chat_content_part::Content::ImageUrl(ChatImageUrl {
            url: url.into(),
            detail: detail.map(Into::into),
        })),
    }
}

pub fn image_data_part(
    media_type: impl Into<String>,
    data_base64: impl Into<String>,
    detail: Option<impl Into<String>>,
) -> ChatContentPart {
    ChatContentPart {
        content: Some(chat_content_part::Content::ImageData(ChatImageData {
            media_type: media_type.into(),
            data_base64: data_base64.into(),
            detail: detail.map(Into::into),
        })),
    }
}

pub fn content_parts_text(parts: &[ChatContentPart]) -> String {
    parts
        .iter()
        .filter_map(|part| match part.content.as_ref()? {
            chat_content_part::Content::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub fn chat_message_text(role: impl Into<String>, content: impl Into<String>) -> ChatMessage {
    let content = content.into();
    ChatMessage {
        role: role.into(),
        content_parts: if content.is_empty() {
            Vec::new()
        } else {
            vec![text_part(content)]
        },
        tool_calls: Vec::new(),
        tool_call_id: None,
    }
}

pub trait ChatMessageExt {
    fn text_content(&self) -> String;
    fn is_empty_content(&self) -> bool;
}

impl ChatMessageExt for ChatMessage {
    fn text_content(&self) -> String {
        content_parts_text(&self.content_parts)
    }

    fn is_empty_content(&self) -> bool {
        self.content_parts
            .iter()
            .all(|part| match part.content.as_ref() {
                Some(chat_content_part::Content::Text(text)) => text.is_empty(),
                None => true,
                _ => false,
            })
    }
}

pub fn text_delta_event(text: impl Into<String>) -> ChatStreamEvent {
    ChatStreamEvent {
        event: Some(chat_stream_event::Event::TextDelta(text.into())),
    }
}

pub fn reasoning_delta_event(reasoning: impl Into<String>) -> ChatStreamEvent {
    ChatStreamEvent {
        event: Some(chat_stream_event::Event::ReasoningDelta(reasoning.into())),
    }
}

pub fn tool_call_delta_event(delta: ToolCallDelta) -> ChatStreamEvent {
    ChatStreamEvent {
        event: Some(chat_stream_event::Event::ToolCallDelta(delta)),
    }
}

pub fn usage_event(usage: ChatUsage) -> ChatStreamEvent {
    ChatStreamEvent {
        event: Some(chat_stream_event::Event::Usage(usage)),
    }
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
