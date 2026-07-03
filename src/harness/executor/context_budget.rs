// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::runtime::LoopMessage;
use crate::harness::llm::{chat_content_part, text_part, ChatContentPart};
use serde_json::Value;

const INLINE_IMAGE_CONTEXT_WEIGHT: usize = 4_000;
const TOOL_RESULT_STORAGE_PREVIEW_CHARS: usize = 12_000;

#[derive(Debug, Clone)]
enum HistorySegment {
    Message(LoopMessage),
    ToolInteraction {
        assistant: LoopMessage,
        tool_results: Vec<LoopMessage>,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct ContextBudget {
    pub total_chars: usize,
    pub max_message_chars: usize,
    pub max_tool_result_chars: usize,
    pub max_tool_argument_chars: usize,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            total_chars: env_usize("TALON_LLM_HISTORY_MAX_CHARS", 96_000),
            max_message_chars: env_usize("TALON_LLM_MESSAGE_MAX_CHARS", 12_000),
            max_tool_result_chars: env_usize("TALON_LLM_TOOL_RESULT_MAX_CHARS", 128_000),
            max_tool_argument_chars: env_usize("TALON_LLM_TOOL_ARGUMENT_MAX_CHARS", 4_000),
        }
    }
}

pub fn tool_result_preview(result: &str) -> String {
    truncate_middle(
        result,
        env_usize(
            "TALON_SESSION_TOOL_RESULT_PREVIEW_CHARS",
            TOOL_RESULT_STORAGE_PREVIEW_CHARS,
        ),
    )
}

pub fn compact_history_for_llm(history: &[LoopMessage]) -> Vec<LoopMessage> {
    compact_history_for_llm_with_budget(history, ContextBudget::default())
}

pub fn compact_history_for_llm_with_budget(
    history: &[LoopMessage],
    budget: ContextBudget,
) -> Vec<LoopMessage> {
    let raw_total_chars = history.iter().map(serialized_message_weight).sum::<usize>();

    // The common path should be lossless. Do not normalize tool output, tool
    // args, or assistant text until the conversation actually exceeds budget.
    if raw_total_chars <= budget.total_chars && tool_history_is_consistent(history) {
        return history.to_vec();
    }

    let mut segments = segment_history(history, budget);
    debug_assert!(
        tool_history_is_consistent(&flatten_segments(&segments)),
        "segmented replay history must preserve valid tool-call structure"
    );

    // First compact older segments in place. Only after every older segment has
    // been squeezed do we omit whole segments from the front of the transcript.
    for index in 0..segments.len() {
        if total_segment_weight(&segments) <= budget.total_chars {
            break;
        }
        if segments[index].is_system_only() {
            continue;
        }
        let compacted = segments[index].compact(budget);
        if compacted.weight() < segments[index].weight() {
            segments[index] = compacted;
        }
    }

    let mut system_messages = segments
        .iter()
        .filter_map(|segment| match segment {
            HistorySegment::Message(message) if message.role == "system" => Some(message.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut non_system_segments = segments
        .into_iter()
        .filter(|segment| !segment.is_system_only())
        .collect::<Vec<_>>();
    let mut omitted = 0usize;

    loop {
        let marker = omitted_marker(omitted);
        let total_chars = system_messages
            .iter()
            .map(serialized_message_weight)
            .sum::<usize>()
            + marker.as_ref().map(serialized_message_weight).unwrap_or(0)
            + total_segment_weight(&non_system_segments);
        if total_chars <= budget.total_chars {
            if let Some(marker) = marker {
                system_messages.push(marker);
            }
            break;
        }

        if non_system_segments.len() <= 1 {
            if let Some(segment) = non_system_segments.first_mut() {
                let fitted = segment.force_fit(budget);
                if fitted.weight() < segment.weight() {
                    *segment = fitted;
                    continue;
                }
            }
            if let Some(marker) = marker {
                system_messages.push(marker);
            }
            break;
        }

        let removed = non_system_segments.remove(0);
        if matches!(removed, HistorySegment::ToolInteraction { .. }) {
            non_system_segments.insert(0, removed.force_fit(budget));
        } else {
            omitted += removed.message_count();
        }
    }

    let mut compacted = system_messages;
    compacted.extend(flatten_segments(&non_system_segments));
    debug_assert!(
        tool_history_is_consistent(&compacted),
        "compacted replay history must preserve valid tool-call structure"
    );
    compacted
}

fn compact_loop_message(message: &LoopMessage, budget: ContextBudget) -> LoopMessage {
    let max_chars = if message.role == "tool" {
        budget.max_tool_result_chars
    } else {
        budget.max_message_chars
    };
    let content_parts = if message.role == "tool" {
        text_parts(compact_tool_result_for_llm(&message.text_content(), budget))
    } else if message.role == "system" {
        message.content_parts.clone()
    } else {
        truncate_text_parts(&message.content_parts, max_chars)
    };

    let tool_calls = message.tool_calls.as_ref().map(|calls| {
        calls
            .iter()
            .map(|call| crate::harness::llm::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: truncate_middle(&call.arguments, budget.max_tool_argument_chars),
            })
            .collect::<Vec<_>>()
    });

    LoopMessage {
        role: message.role.clone(),
        content_parts,
        tool_calls,
        tool_call_id: message.tool_call_id.clone(),
    }
}

fn force_fit_message(message: &LoopMessage, budget: ContextBudget) -> LoopMessage {
    let mut compacted = compact_loop_message(message, budget);
    let metadata_weight = message.role.len()
        + message
            .tool_calls
            .as_ref()
            .map(|calls| {
                calls
                    .iter()
                    .map(|call| call.id.len() + call.name.len() + call.arguments.len())
                    .sum::<usize>()
            })
            .unwrap_or(0)
        + message
            .tool_call_id
            .as_ref()
            .map(|id| id.len())
            .unwrap_or(0);
    let allowed_chars = budget
        .total_chars
        .saturating_sub(metadata_weight)
        .min(budget.max_message_chars.max(512));
    compacted.content_parts = fit_content_parts_to_weight(&compacted.content_parts, allowed_chars);
    compacted
}

fn serialized_message_weight(message: &LoopMessage) -> usize {
    let tool_call_weight = message
        .tool_calls
        .as_ref()
        .map(|calls| {
            calls
                .iter()
                .map(|call| call.id.len() + call.name.len() + call.arguments.len())
                .sum::<usize>()
        })
        .unwrap_or(0);
    message.role.len()
        + content_parts_weight(&message.content_parts)
        + tool_call_weight
        + message
            .tool_call_id
            .as_ref()
            .map(|id| id.len())
            .unwrap_or(0)
}

fn segment_history(history: &[LoopMessage], budget: ContextBudget) -> Vec<HistorySegment> {
    let mut segments = Vec::new();
    let mut index = 0usize;

    while index < history.len() {
        let message = &history[index];

        if message.role == "tool" {
            segments.push(HistorySegment::Message(tool_segment_summary(
                None,
                std::slice::from_ref(message),
                budget,
            )));
            index += 1;
            continue;
        }

        if !message
            .tool_calls
            .as_ref()
            .is_some_and(|calls| !calls.is_empty())
        {
            segments.push(HistorySegment::Message(message.clone()));
            index += 1;
            continue;
        }

        let expected_ids = message
            .tool_calls
            .as_ref()
            .map(|calls| calls.iter().map(|call| call.id.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut tool_results = Vec::new();
        let mut cursor = index + 1;
        while cursor < history.len() && history[cursor].role == "tool" {
            tool_results.push(history[cursor].clone());
            cursor += 1;
        }

        let has_all_expected = expected_ids.iter().all(|id| {
            tool_results
                .iter()
                .any(|tool_message| tool_message.tool_call_id.as_deref() == Some(id.as_str()))
        });
        let has_only_expected = tool_results.iter().all(|tool_message| {
            tool_message
                .tool_call_id
                .as_ref()
                .is_some_and(|id| expected_ids.iter().any(|expected| expected == id))
        });

        if has_all_expected && has_only_expected {
            segments.push(HistorySegment::ToolInteraction {
                assistant: message.clone(),
                tool_results,
            });
        } else {
            segments.push(HistorySegment::Message(tool_segment_summary(
                Some(message),
                &tool_results,
                budget,
            )));
        }

        index = cursor.max(index + 1);
    }

    segments
}

fn flatten_segments(segments: &[HistorySegment]) -> Vec<LoopMessage> {
    let mut flattened = Vec::new();
    for segment in segments {
        match segment {
            HistorySegment::Message(message) => flattened.push(message.clone()),
            HistorySegment::ToolInteraction {
                assistant,
                tool_results,
            } => {
                flattened.push(assistant.clone());
                flattened.extend(tool_results.clone());
            }
        }
    }
    flattened
}

fn total_segment_weight(segments: &[HistorySegment]) -> usize {
    segments.iter().map(HistorySegment::weight).sum()
}

fn omitted_marker(omitted: usize) -> Option<LoopMessage> {
    if omitted == 0 {
        return None;
    }
    Some(LoopMessage::text(
        "system",
        format!(
            "[{} earlier messages omitted to stay within Talon context budget.]",
            omitted
        ),
    ))
}

fn tool_segment_summary(
    assistant: Option<&LoopMessage>,
    tool_results: &[LoopMessage],
    budget: ContextBudget,
) -> LoopMessage {
    let mut parts = Vec::new();
    if let Some(assistant) = assistant {
        let assistant_text = assistant.text_content();
        if !assistant_text.trim().is_empty() {
            parts.push(truncate_middle(
                &assistant_text,
                budget.max_message_chars / 2,
            ));
        }
        if let Some(tool_calls) = &assistant.tool_calls {
            let names = tool_calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>();
            if !names.is_empty() {
                parts.push(format!(
                    "[Prior tool interaction omitted to preserve a valid tool transcript. Tool calls: {}.]",
                    names.join(", ")
                ));
            }
        }
    } else {
        parts.push(
            "[Prior orphaned tool result omitted to preserve a valid tool transcript.]".to_string(),
        );
    }

    if parts.is_empty() && !tool_results.is_empty() {
        parts.push(
            "[Prior tool interaction omitted to preserve a valid tool transcript.]".to_string(),
        );
    }

    let content = truncate_middle(&parts.join("\n\n"), budget.max_message_chars);
    LoopMessage::text(
        assistant
            .map(|message| message.role.clone())
            .unwrap_or_else(|| "assistant".to_string()),
        content,
    )
}

fn text_parts(text: String) -> Vec<ChatContentPart> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![text_part(text)]
    }
}

fn truncate_text_parts(parts: &[ChatContentPart], max_chars: usize) -> Vec<ChatContentPart> {
    let total_text_len = parts
        .iter()
        .filter_map(|part| match part.content.as_ref() {
            Some(chat_content_part::Content::Text(text)) => Some(text.len()),
            _ => None,
        })
        .sum::<usize>();
    if total_text_len <= max_chars {
        return parts.to_vec();
    }

    let mut remaining = max_chars;
    let mut truncated = Vec::with_capacity(parts.len());
    for part in parts {
        match part.content.as_ref() {
            Some(chat_content_part::Content::Text(text)) => {
                if remaining == 0 {
                    continue;
                }
                let next = truncate_middle(text, remaining);
                remaining = remaining.saturating_sub(next.len());
                if !next.is_empty() {
                    truncated.push(text_part(next));
                }
            }
            _ => truncated.push(part.clone()),
        }
    }
    truncated
}

fn fit_content_parts_to_weight(
    parts: &[ChatContentPart],
    max_weight: usize,
) -> Vec<ChatContentPart> {
    let mut remaining = max_weight;
    let mut fitted = Vec::with_capacity(parts.len());

    for part in parts {
        match part.content.as_ref() {
            Some(chat_content_part::Content::Text(text)) => {
                if remaining == 0 {
                    continue;
                }
                let next = truncate_middle(text, remaining);
                remaining = remaining.saturating_sub(next.len());
                if !next.is_empty() {
                    fitted.push(text_part(next));
                }
            }
            Some(chat_content_part::Content::ImageUrl(_))
            | Some(chat_content_part::Content::ImageData(_)) => {
                let weight = content_part_weight(part);
                if weight <= remaining {
                    fitted.push(part.clone());
                    remaining = remaining.saturating_sub(weight);
                    continue;
                }

                let marker = match part.content.as_ref() {
                    Some(chat_content_part::Content::ImageUrl(_)) => {
                        "[Image URL omitted to stay within Talon context budget.]"
                    }
                    Some(chat_content_part::Content::ImageData(_)) => {
                        "[Image omitted to stay within Talon context budget.]"
                    }
                    _ => unreachable!(),
                };
                if marker.len() <= remaining {
                    fitted.push(text_part(marker.to_string()));
                    remaining = remaining.saturating_sub(marker.len());
                }
            }
            None => {}
        }
    }

    fitted
}

fn content_part_weight(part: &ChatContentPart) -> usize {
    match part.content.as_ref() {
        Some(chat_content_part::Content::Text(text)) => text.len(),
        Some(chat_content_part::Content::ImageUrl(image)) => {
            image.url.len()
                + image
                    .detail
                    .as_ref()
                    .map(|detail| detail.len())
                    .unwrap_or(0)
        }
        Some(chat_content_part::Content::ImageData(image)) => {
            INLINE_IMAGE_CONTEXT_WEIGHT
                + image.media_type.len()
                + image
                    .detail
                    .as_ref()
                    .map(|detail| detail.len())
                    .unwrap_or(0)
        }
        None => 0,
    }
}

fn content_parts_weight(parts: &[ChatContentPart]) -> usize {
    parts.iter().map(content_part_weight).sum()
}

impl HistorySegment {
    fn is_system_only(&self) -> bool {
        matches!(self, HistorySegment::Message(message) if message.role == "system")
    }

    fn message_count(&self) -> usize {
        match self {
            HistorySegment::Message(_) => 1,
            HistorySegment::ToolInteraction { tool_results, .. } => 1 + tool_results.len(),
        }
    }

    fn weight(&self) -> usize {
        match self {
            HistorySegment::Message(message) => serialized_message_weight(message),
            HistorySegment::ToolInteraction {
                assistant,
                tool_results,
            } => {
                serialized_message_weight(assistant)
                    + tool_results
                        .iter()
                        .map(serialized_message_weight)
                        .sum::<usize>()
            }
        }
    }

    fn force_fit(&self, budget: ContextBudget) -> HistorySegment {
        match self {
            HistorySegment::Message(message) => {
                HistorySegment::Message(force_fit_message(message, budget))
            }
            HistorySegment::ToolInteraction {
                assistant,
                tool_results,
            } => HistorySegment::Message(force_fit_message(
                &tool_segment_summary(Some(assistant), tool_results, budget),
                budget,
            )),
        }
    }

    fn compact(&self, budget: ContextBudget) -> HistorySegment {
        match self {
            HistorySegment::Message(message) => {
                HistorySegment::Message(compact_loop_message(message, budget))
            }
            HistorySegment::ToolInteraction {
                assistant,
                tool_results,
            } => HistorySegment::ToolInteraction {
                assistant: compact_loop_message(assistant, budget),
                tool_results: tool_results
                    .iter()
                    .map(|message| compact_loop_message(message, budget))
                    .collect(),
            },
        }
    }
}

fn tool_history_is_consistent(history: &[LoopMessage]) -> bool {
    let mut pending_call_ids: Vec<String> = Vec::new();

    for message in history {
        if let Some(tool_calls) = &message.tool_calls {
            if !tool_calls.is_empty() {
                pending_call_ids.extend(tool_calls.iter().map(|call| call.id.clone()));
            }
        }

        if message.role == "tool" {
            let Some(tool_call_id) = message.tool_call_id.as_ref() else {
                return false;
            };
            let Some(position) = pending_call_ids.iter().position(|id| id == tool_call_id) else {
                return false;
            };
            pending_call_ids.remove(position);
        }
    }

    pending_call_ids.is_empty()
}

fn compact_tool_result_for_llm(result: &str, budget: ContextBudget) -> String {
    if result.len() <= budget.max_tool_result_chars {
        return result.to_string();
    }

    let Ok(mut value) = serde_json::from_str::<Value>(result) else {
        return truncate_middle(result, budget.max_tool_result_chars);
    };
    let Ok(rendered) = serde_json::to_string_pretty(&value) else {
        return truncate_middle(result, budget.max_tool_result_chars);
    };
    if rendered.len() <= budget.max_tool_result_chars {
        return rendered;
    }

    // Keep JSON shape intact: trim only string leaves, largest first, and
    // rerender after each trim because JSON escaping changes the final size.
    for _ in 0..128 {
        let rendered = serde_json::to_string_pretty(&value).unwrap_or_else(|_| result.to_string());
        if rendered.len() <= budget.max_tool_result_chars {
            return rendered;
        }

        let Some(largest) = largest_string_leaf_path(&value) else {
            return truncate_middle(&rendered, budget.max_tool_result_chars);
        };
        let excess = rendered.len().saturating_sub(budget.max_tool_result_chars);
        let Some(text) = string_leaf_mut(&mut value, &largest) else {
            return truncate_middle(&rendered, budget.max_tool_result_chars);
        };
        let next_len = text
            .chars()
            .count()
            .saturating_sub(excess.saturating_add(64));
        let truncated = truncate_middle(text, next_len);
        if truncated == *text {
            return truncate_middle(&rendered, budget.max_tool_result_chars);
        }
        *text = truncated;
    }

    serde_json::to_string_pretty(&value)
        .ok()
        .map(|rendered| truncate_middle(&rendered, budget.max_tool_result_chars))
        .unwrap_or_else(|| truncate_middle(result, budget.max_tool_result_chars))
}

#[derive(Clone)]
enum JsonPathElement {
    Key(String),
    Index(usize),
}

fn largest_string_leaf_path(value: &Value) -> Option<Vec<JsonPathElement>> {
    fn visit(
        value: &Value,
        path: &mut Vec<JsonPathElement>,
        largest: &mut Option<(usize, Vec<JsonPathElement>)>,
    ) {
        match value {
            Value::String(text) => {
                let len = text.chars().count();
                if largest
                    .as_ref()
                    .is_none_or(|(current_len, _)| len > *current_len)
                {
                    *largest = Some((len, path.clone()));
                }
            }
            Value::Array(array) => {
                for (index, child) in array.iter().enumerate() {
                    path.push(JsonPathElement::Index(index));
                    visit(child, path, largest);
                    path.pop();
                }
            }
            Value::Object(object) => {
                for (key, child) in object {
                    path.push(JsonPathElement::Key(key.clone()));
                    visit(child, path, largest);
                    path.pop();
                }
            }
            _ => {}
        }
    }

    let mut largest = None;
    visit(value, &mut Vec::new(), &mut largest);
    largest.map(|(_, path)| path)
}

fn string_leaf_mut<'a>(
    mut value: &'a mut Value,
    path: &[JsonPathElement],
) -> Option<&'a mut String> {
    for element in path {
        match element {
            JsonPathElement::Key(key) => {
                value = value.as_object_mut()?.get_mut(key)?;
            }
            JsonPathElement::Index(index) => {
                value = value.as_array_mut()?.get_mut(*index)?;
            }
        }
    }
    value.as_str()?;
    match value {
        Value::String(text) => Some(text),
        _ => None,
    }
}

fn truncate_middle(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return text.to_string();
    }

    if max_chars <= 32 {
        return chars.into_iter().take(max_chars).collect();
    }

    let omitted = chars.len() - max_chars;
    let marker = format!("\n...[{omitted} chars omitted]...\n");
    let marker_len = marker.chars().count();
    if marker_len >= max_chars {
        return chars.into_iter().take(max_chars).collect();
    }

    let remaining = max_chars - marker_len;
    let prefix_len = remaining * 2 / 3;
    let suffix_len = remaining.saturating_sub(prefix_len);
    let prefix = chars.iter().take(prefix_len).collect::<String>();
    let suffix = chars
        .iter()
        .skip(chars.len().saturating_sub(suffix_len))
        .collect::<String>();
    format!("{prefix}{marker}{suffix}")
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::{
        compact_history_for_llm_with_budget, serialized_message_weight, tool_history_is_consistent,
        ContextBudget,
    };
    use crate::harness::executor::LoopMessage;
    use crate::harness::llm::{chat_content_part, image_data_part, text_part, ToolCall};
    use serde::Deserialize;

    fn budget() -> ContextBudget {
        ContextBudget {
            total_chars: 800,
            max_message_chars: 200,
            max_tool_result_chars: 180,
            max_tool_argument_chars: 120,
        }
    }

    fn prod_novita_budget() -> ContextBudget {
        ContextBudget {
            total_chars: 48_000,
            max_message_chars: 8_000,
            max_tool_result_chars: 4_000,
            max_tool_argument_chars: 2_000,
        }
    }

    fn message(role: impl Into<String>, content: impl Into<String>) -> LoopMessage {
        LoopMessage::text(role, content)
    }

    #[derive(Deserialize)]
    struct FixtureLoopMessage {
        role: String,
        content: String,
    }

    fn load_session_019e052a_history() -> Vec<LoopMessage> {
        let fixture = include_str!("testdata/019e052a_loop_history.json");
        let parsed: Vec<FixtureLoopMessage> =
            serde_json::from_str(fixture).expect("session fixture should parse");
        parsed
            .into_iter()
            .map(|message| LoopMessage::text(message.role, message.content))
            .collect()
    }

    #[test]
    fn compact_history_is_lossless_when_total_history_fits() {
        let stdout = "inspection report text ".repeat(120);
        let tool_output = format!(
            r#"{{"data":{{"stdout":{},"status":"ok"}}}}"#,
            serde_json::to_string(&stdout).unwrap()
        );
        let mut assistant = message("assistant", "");
        assistant.tool_calls = Some(vec![ToolCall {
            id: "tool-1".to_string(),
            name: "extract".to_string(),
            arguments: "{}".to_string(),
        }]);
        let mut tool = message("tool", tool_output.clone());
        tool.tool_call_id = Some("tool-1".to_string());
        let history = vec![
            message("system", "sys"),
            assistant,
            tool,
            message("user", "continue"),
        ];
        let relaxed_budget = ContextBudget {
            total_chars: 20_000,
            max_tool_result_chars: 500,
            ..budget()
        };

        let compacted = compact_history_for_llm_with_budget(&history, relaxed_budget);

        assert_eq!(compacted, history);
        assert!(compacted[2].text_content().contains(&stdout));
    }

    #[test]
    fn over_budget_tool_json_trims_large_string_leaves_without_changing_shape() {
        let tool_output = format!(
            r#"{{"data":{{"stdout":{},"stderr":"small","status":"ok"}},"items":[{{"path":"report.txt","content":"tiny"}}]}}"#,
            serde_json::to_string(&"A".repeat(2_000)).unwrap()
        );
        let mut assistant = message("assistant", "");
        assistant.tool_calls = Some(vec![ToolCall {
            id: "tool-1".to_string(),
            name: "extract".to_string(),
            arguments: "{}".to_string(),
        }]);
        let mut tool = message("tool", tool_output);
        tool.tool_call_id = Some("tool-1".to_string());
        let compact_budget = ContextBudget {
            total_chars: 900,
            max_tool_result_chars: 360,
            max_tool_argument_chars: 120,
            ..budget()
        };

        let compacted = compact_history_for_llm_with_budget(&[assistant, tool], compact_budget);
        let tool_message = compacted
            .iter()
            .find(|message| message.role == "tool")
            .expect("tool result should remain when compacted interaction fits");
        let parsed: serde_json::Value = serde_json::from_str(&tool_message.text_content())
            .expect("compacted tool result should remain JSON");

        assert!(tool_message.text_content().len() <= compact_budget.max_tool_result_chars);
        assert_eq!(parsed["data"]["stderr"], "small");
        assert_eq!(parsed["data"]["status"], "ok");
        assert_eq!(parsed["items"][0]["path"], "report.txt");
        assert!(parsed["data"]["stdout"]
            .as_str()
            .unwrap()
            .contains("chars omitted"));
    }

    #[test]
    fn compact_history_keeps_recent_messages_within_budget() {
        let history = vec![
            message("system", "sys".repeat(40)),
            message("assistant", "A".repeat(500)),
            {
                let mut message =
                    message("tool", format!(r#"{{"payload":"{}"}}"#, "B".repeat(500)));
                message.tool_call_id = Some("tool-1".to_string());
                message
            },
            message("user", "latest question"),
        ];

        let compacted = compact_history_for_llm_with_budget(&history, budget());
        let combined_len = compacted
            .iter()
            .map(|m| m.text_content().len())
            .sum::<usize>();

        assert!(combined_len <= budget().total_chars + 128);
        assert_eq!(compacted.last().unwrap().text_content(), "latest question");
        assert!(compacted
            .iter()
            .any(|m| m.text_content().contains("omitted")));
    }

    #[test]
    fn compact_history_preserves_multimodal_parts_when_message_is_kept() {
        let mut user = message("user", "");
        user.content_parts = vec![
            text_part("describe this"),
            image_data_part("image/png", "x".repeat(200_000), None::<String>),
        ];
        let history = vec![message("system", "sys"), user];

        let compacted = compact_history_for_llm_with_budget(
            &history,
            ContextBudget {
                total_chars: 5_000,
                max_message_chars: 5_000,
                ..budget()
            },
        );

        assert!(compacted.iter().any(|message| {
            message.role == "user"
                && message.content_parts.iter().any(|part| {
                    matches!(
                        part.content.as_ref(),
                        Some(chat_content_part::Content::ImageData(image))
                            if image.media_type == "image/png"
                    )
                })
        }));
    }

    #[test]
    fn compact_history_force_fit_bounds_multimodal_message_weight() {
        let mut user = message("user", "");
        user.content_parts = vec![
            text_part("please inspect this image carefully"),
            image_data_part("image/png", "x".repeat(200_000), None::<String>),
        ];
        let tiny_budget = ContextBudget {
            total_chars: 80,
            max_message_chars: 80,
            ..budget()
        };

        let compacted = compact_history_for_llm_with_budget(&[user], tiny_budget);
        let total_weight = compacted
            .iter()
            .map(serialized_message_weight)
            .sum::<usize>();

        assert!(total_weight <= tiny_budget.total_chars);
        assert!(compacted
            .iter()
            .flat_map(|message| &message.content_parts)
            .all(|part| {
                !matches!(
                    part.content.as_ref(),
                    Some(chat_content_part::Content::ImageData(_))
                )
            }));
    }

    #[test]
    fn compact_history_preserves_complete_tool_interaction() {
        let history = vec![
            message("user", "Inspect the footer."),
            {
                let mut message = message("assistant", "");
                message.tool_calls = Some(vec![ToolCall {
                    id: "tool-1".to_string(),
                    name: "mcp_github_get_file_contents".to_string(),
                    arguments: "{\"path\":\"Footer.tsx\"}".to_string(),
                }]);
                message
            },
            {
                let mut message = message("tool", "{\"content\":\"export function Footer() {}\"}");
                message.tool_call_id = Some("tool-1".to_string());
                message
            },
            message("user", "Continue"),
        ];

        let compacted = compact_history_for_llm_with_budget(&history, budget());

        assert!(tool_history_is_consistent(&compacted));
        assert!(compacted.iter().any(|message| {
            message
                .tool_calls
                .as_ref()
                .is_some_and(|calls| calls.iter().any(|call| call.id == "tool-1"))
        }));
        assert!(compacted.iter().any(|message| {
            message.role == "tool" && message.tool_call_id.as_deref() == Some("tool-1")
        }));
    }

    #[test]
    fn compact_history_compacts_oversized_tool_interaction_without_splitting_it() {
        let history = vec![
            {
                let mut message = message("assistant", "");
                message.tool_calls = Some(vec![ToolCall {
                    id: "tool-1".to_string(),
                    name: "mcp_github_search_code".to_string(),
                    arguments: "x".repeat(1_000),
                }]);
                message
            },
            {
                let mut message = message(
                    "tool",
                    format!("{{\"items\":[{{\"content\":\"{}\"}}]}}", "y".repeat(4_000)),
                );
                message.tool_call_id = Some("tool-1".to_string());
                message
            },
            message("user", "Continue"),
        ];
        let tiny_budget = ContextBudget {
            total_chars: 180,
            max_message_chars: 120,
            max_tool_result_chars: 80,
            max_tool_argument_chars: 60,
        };

        let compacted = compact_history_for_llm_with_budget(&history, tiny_budget);

        assert!(tool_history_is_consistent(&compacted));
        assert!(compacted.iter().any(|message| message.role == "tool"));
        assert!(compacted.iter().any(|message| message
            .tool_calls
            .as_ref()
            .is_some_and(|calls| calls.iter().any(|call| call.id == "tool-1"))));
        assert!(compacted
            .iter()
            .filter(|message| message.role == "tool")
            .all(|message| message.text_content().len() <= tiny_budget.max_tool_result_chars));
    }

    #[test]
    fn compact_history_degrades_orphaned_tool_messages() {
        let history = vec![
            {
                let mut message = message("tool", "{\"content\":\"orphan\"}");
                message.tool_call_id = Some("tool-orphan".to_string());
                message
            },
            message("user", "Continue"),
        ];

        let compacted = compact_history_for_llm_with_budget(&history, budget());

        assert!(tool_history_is_consistent(&compacted));
        assert!(!compacted.iter().any(|message| message.role == "tool"));
        assert!(compacted
            .iter()
            .any(|message| message.text_content().contains("orphaned tool result")));
    }

    #[test]
    fn downloaded_session_019e052a_is_compacted_under_prod_novita_char_limits() {
        let history = load_session_019e052a_history();
        let budget = prod_novita_budget();

        let compacted = compact_history_for_llm_with_budget(&history, budget);
        let total_weight = compacted
            .iter()
            .map(serialized_message_weight)
            .sum::<usize>();

        assert!(
            total_weight <= budget.total_chars,
            "compacted replay weight {} exceeded budget {}",
            total_weight,
            budget.total_chars
        );
        assert!(
            compacted
                .iter()
                .filter(|message| message.role == "tool")
                .all(|message| message.text_content().len() <= budget.max_tool_result_chars),
            "tool output exceeded max_tool_result_chars"
        );
        assert!(
            compacted
                .iter()
                .filter(|message| message.role != "tool" && message.role != "system")
                .all(|message| message.text_content().len() <= budget.max_message_chars),
            "non-tool message exceeded max_message_chars"
        );
        assert!(
            compacted
                .iter()
                .any(|message| message.text_content().contains("omitted")),
            "expected older replay history to be omitted for this downloaded session"
        );
        assert!(
            tool_history_is_consistent(&compacted),
            "compacted replay should preserve valid tool-call structure"
        );
        assert_eq!(
            compacted.last().map(|message| message.text_content()),
            Some(String::new()),
            "latest assistant message from the downloaded session should remain in replay tail"
        );
    }
}
