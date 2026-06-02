use std::collections::BTreeMap;

use serde_json::Value;

use crate::model::{ChatMessage, ToolCall, Usage};

/// Result of assembling an OpenAI-compatible SSE stream body.
pub struct AssembledStream {
    pub assistant: Option<ChatMessage>,
    pub finish_reason: Option<String>,
    pub usage: Usage,
    /// True if the `[DONE]` sentinel was observed.
    pub saw_done: bool,
}

/// If `body_val` carries SSE text (either as `{ "data": "data: ..." }` or as a
/// raw string starting with `data:`), return that text.
pub fn extract_sse_text(body_val: &Value) -> Option<String> {
    if let Some(s) = body_val.as_str() {
        let t = s.trim_start();
        if t.starts_with("data:") {
            return Some(s.to_string());
        }
        return None;
    }
    let data = body_val.get("data")?.as_str()?;
    if data.trim_start().starts_with("data:") {
        Some(data.to_string())
    } else {
        None
    }
}

/// Reassemble an OpenAI-compatible SSE chat completion stream into a single
/// assistant `ChatMessage`. Handles content deltas, tool-call deltas (merged
/// by index), the final `finish_reason`, and `usage` (including providers
/// like Groq that embed usage in the last delta chunk).
///
/// TODO: Anthropic streaming (`event: content_block_delta`, etc.) is not yet
/// supported — those payloads use named events instead of plain `choices`.
pub fn assemble_openai_stream(raw: &str) -> AssembledStream {
    let mut role = String::from("assistant");
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_calls: BTreeMap<u64, ToolCallBuilder> = BTreeMap::new();
    let mut finish_reason: Option<String> = None;
    let mut usage = Usage::default();
    let mut saw_done = false;

    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        let payload = match line.strip_prefix("data:") {
            Some(p) => p.trim_start(),
            None => continue,
        };
        if payload.is_empty() {
            continue;
        }
        if payload == "[DONE]" {
            saw_done = true;
            break;
        }
        let chunk: Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if let Some(u) = chunk.get("usage").filter(|v| v.is_object()) {
            usage = Usage {
                prompt_tokens: u.get("prompt_tokens").and_then(|x| x.as_u64()),
                completion_tokens: u.get("completion_tokens").and_then(|x| x.as_u64()),
                total_tokens: u.get("total_tokens").and_then(|x| x.as_u64()),
            };
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
        else {
            continue;
        };

        if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            finish_reason = Some(fr.to_string());
        }

        let Some(delta) = choice.get("delta") else {
            continue;
        };

        if let Some(r) = delta.get("role").and_then(|v| v.as_str()) {
            role = r.to_string();
        }
        if let Some(c) = delta.get("content").and_then(|v| v.as_str()) {
            content.push_str(c);
        }
        // Groq / OpenRouter expose reasoning deltas as `delta.reasoning`;
        // DeepSeek / xAI / NIM use `delta.reasoning_content`.
        if let Some(r) = delta.get("reasoning").and_then(|v| v.as_str()) {
            reasoning.push_str(r);
        }
        if let Some(r) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
            reasoning.push_str(r);
        }
        if let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in tcs {
                let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
                let entry = tool_calls.entry(idx).or_default();
                if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                    if !id.is_empty() {
                        entry.id = id.to_string();
                    }
                }
                if let Some(f) = tc.get("function") {
                    if let Some(name) = f.get("name").and_then(|v| v.as_str()) {
                        if !name.is_empty() {
                            entry.name = name.to_string();
                        }
                    }
                    if let Some(args) = f.get("arguments").and_then(|v| v.as_str()) {
                        entry.arguments.push_str(args);
                    }
                }
            }
        }
    }

    let tool_calls_vec: Vec<ToolCall> = tool_calls
        .into_values()
        .map(|b| ToolCall {
            id: b.id,
            name: b.name,
            arguments: b.arguments,
        })
        .collect();

    let has_content = !content.is_empty();
    let has_tools = !tool_calls_vec.is_empty();
    let has_reasoning = !reasoning.is_empty();
    let assistant = if has_content || has_tools || has_reasoning {
        // Inline `<think>...</think>` may still appear in the assembled content
        // for local models that emit it as plain text rather than a dedicated
        // delta field. Move it into reasoning.
        let (visible, extra_thought) = super::openai_audit::split_think_tags_public(&content);
        let final_content = if has_content { visible } else { String::new() };
        let mut final_reasoning = reasoning;
        if !extra_thought.is_empty() {
            if !final_reasoning.is_empty() {
                final_reasoning.push_str("\n\n");
            }
            final_reasoning.push_str(&extra_thought);
        }
        Some(ChatMessage {
            role,
            content: if final_content.is_empty() {
                None
            } else {
                Some(final_content)
            },
            tool_calls: if has_tools { Some(tool_calls_vec) } else { None },
            reasoning: if final_reasoning.is_empty() {
                None
            } else {
                Some(final_reasoning)
            },
            ..Default::default()
        })
    } else {
        None
    };

    AssembledStream {
        assistant,
        finish_reason,
        usage,
        saw_done,
    }
}

#[derive(Default)]
struct ToolCallBuilder {
    id: String,
    name: String,
    arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_sse_in_object() {
        let v: Value = serde_json::json!({
            "data": "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n",
            "size": 42,
        });
        assert!(extract_sse_text(&v).is_some());
    }

    #[test]
    fn detects_sse_in_raw_string() {
        let v = Value::String("data: {\"choices\":[]}\n\ndata: [DONE]".into());
        assert!(extract_sse_text(&v).is_some());
    }

    #[test]
    fn ignores_non_sse() {
        let v: Value = serde_json::json!({ "choices": [] });
        assert!(extract_sse_text(&v).is_none());
    }

    #[test]
    fn assembles_content_deltas() {
        let raw = concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"El\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" SDK\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" rocks\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":3,\"total_tokens\":13}}\n\n",
            "data: [DONE]\n\n",
        );
        let out = assemble_openai_stream(raw);
        let msg = out.assistant.expect("assistant present");
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content.as_deref(), Some("El SDK rocks"));
        assert_eq!(out.finish_reason.as_deref(), Some("stop"));
        assert_eq!(out.usage.completion_tokens, Some(3));
        assert!(out.saw_done);
    }

    #[test]
    fn assembles_tool_call_deltas() {
        let raw = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"grep\",\"arguments\":\"{\\\"pat\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"tern\\\":\\\"x\\\"}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let out = assemble_openai_stream(raw);
        let msg = out.assistant.expect("assistant present");
        let tcs = msg.tool_calls.expect("tool_calls present");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_1");
        assert_eq!(tcs[0].name, "grep");
        assert_eq!(tcs[0].arguments, "{\"pattern\":\"x\"}");
        assert_eq!(out.finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn handles_missing_done_sentinel() {
        let raw = "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n";
        let out = assemble_openai_stream(raw);
        assert!(!out.saw_done);
        assert_eq!(
            out.assistant.unwrap().content.as_deref(),
            Some("partial")
        );
    }
}
