use serde_json::Value;

use crate::model::{
    ChatMessage, LogEvent, Provider, RequestPayload, ResponsePayload, ToolCall, Upstream, Usage,
};

use super::sse;
use super::ParseOutcome;

/// Top-level entry: dispatch between JSONL and JSON array based on the first
/// non-whitespace character.
pub fn parse(content: &str) -> ParseOutcome {
    let trimmed = content.trim_start();
    if trimmed.starts_with('[') {
        parse_json_array(content)
    } else {
        parse_jsonl(content)
    }
}

pub fn parse_jsonl(content: &str) -> ParseOutcome {
    let mut events = Vec::new();
    let mut warnings = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(v) => match record_to_event(&v) {
                Ok(e) => events.push(e),
                Err(err) => warnings.push(format!("line {}: {}", idx + 1, err)),
            },
            Err(e) => warnings.push(format!("line {}: invalid json: {}", idx + 1, e)),
        }
    }
    ParseOutcome { events, warnings }
}

pub fn parse_json_array(content: &str) -> ParseOutcome {
    let mut events = Vec::new();
    let mut warnings = Vec::new();
    match serde_json::from_str::<Value>(content) {
        Ok(Value::Array(arr)) => {
            for (idx, v) in arr.iter().enumerate() {
                match record_to_event(v) {
                    Ok(e) => events.push(e),
                    Err(err) => warnings.push(format!("item {}: {}", idx, err)),
                }
            }
        }
        Ok(_) => warnings.push("expected a JSON array".into()),
        Err(e) => warnings.push(format!("invalid json: {}", e)),
    }
    ParseOutcome { events, warnings }
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn record_to_event(v: &Value) -> Result<LogEvent, String> {
    let id = str_field(v, "id").ok_or("missing id")?;
    let ts = str_field(v, "ts")
        .or_else(|| str_field(v, "timestamp"))
        .ok_or("missing ts")?;
    let client = str_field(v, "client").unwrap_or_else(|| "unknown".into());
    let ua = str_field(v, "ua");
    let model_raw = str_field(v, "model").unwrap_or_else(|| "unknown".into());
    let endpoint = str_field(v, "path").unwrap_or_else(|| "/v1/chat/completions".into());
    let status = v.get("status").and_then(|x| x.as_u64()).unwrap_or(0) as u16;
    let duration_ms = v.get("duration_ms").and_then(|x| x.as_u64()).unwrap_or(0);

    let upstream = v.get("upstream").and_then(|u| {
        let url = u.get("url").and_then(|x| x.as_str())?.to_string();
        let status = u.get("status").and_then(|x| x.as_u64()).map(|x| x as u16);
        Some(Upstream { url, status })
    });

    let provider = match Provider::from_prefix(&model_raw) {
        Provider::Unknown => upstream
            .as_ref()
            .and_then(|u| Provider::from_upstream_url(&u.url))
            .unwrap_or(Provider::Unknown),
        p => p,
    };

    let request = parse_request(v.get("req_body"));
    let response = parse_response(v.get("resp_body"));

    Ok(LogEvent {
        id,
        ts,
        client,
        ua,
        provider,
        model: model_raw,
        endpoint,
        request,
        response,
        upstream,
        status,
        duration_ms,
        raw: v.clone(),
    })
}

fn parse_request(body: Option<&Value>) -> RequestPayload {
    let Some(body) = body else {
        return RequestPayload::default();
    };
    // Accept inline JSON object or stringified JSON
    let body_val: Value = if let Some(s) = body.as_str() {
        serde_json::from_str(s).unwrap_or(Value::Null)
    } else {
        body.clone()
    };

    let model = body_val
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let stream = body_val
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let tools_declared = body_val
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|a| a.len() as u32)
        .unwrap_or(0);

    let messages = body_val
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(parse_message).collect())
        .unwrap_or_default();

    RequestPayload {
        model,
        messages,
        stream,
        tools_declared,
    }
}

pub(crate) fn parse_response(body: Option<&Value>) -> ResponsePayload {
    let Some(body) = body else {
        return ResponsePayload::default();
    };
    let body_val: Value = if let Some(s) = body.as_str() {
        serde_json::from_str(s).unwrap_or(Value::Null)
    } else {
        body.clone()
    };

    // Streaming responses are logged as raw SSE text (either a JSON string or
    // an object `{ data, size, truncated }`). Reassemble before falling
    // through to the non-stream matchers.
    if let Some(sse_text) = sse::extract_sse_text(&body_val) {
        let assembled = sse::assemble_openai_stream(&sse_text);
        let truncated = body_val
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let finish_reason = match assembled.finish_reason {
            Some(fr) => Some(fr),
            None if truncated && !assembled.saw_done => Some("truncated".into()),
            None => None,
        };
        return ResponsePayload {
            assistant: assembled.assistant,
            finish_reason,
            usage: assembled.usage,
        };
    }

    let usage = body_val
        .get("usage")
        .map(|u| Usage {
            prompt_tokens: u.get("prompt_tokens").and_then(|x| x.as_u64()),
            completion_tokens: u.get("completion_tokens").and_then(|x| x.as_u64()),
            total_tokens: u.get("total_tokens").and_then(|x| x.as_u64()),
        })
        .unwrap_or_default();

    // OpenAI-shape: choices[0].message + finish_reason
    if let Some(choices) = body_val.get("choices").and_then(|v| v.as_array()) {
        if let Some(first) = choices.first() {
            let finish_reason = first
                .get("finish_reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let assistant = first.get("message").map(parse_message);
            return ResponsePayload {
                assistant,
                finish_reason,
                usage,
            };
        }
    }
    // Anthropic-shape: content array
    if let Some(content) = body_val.get("content").and_then(|v| v.as_array()) {
        let mut text = String::new();
        let mut reasoning_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        for item in content {
            match item.get("type").and_then(|v| v.as_str()) {
                Some("text") => {
                    if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                        text.push_str(t);
                    }
                }
                Some("thinking") => {
                    if let Some(t) = item.get("thinking").and_then(|v| v.as_str()) {
                        reasoning_parts.push(t.to_string());
                    }
                }
                Some("redacted_thinking") => {
                    reasoning_parts.push("[redacted]".to_string());
                }
                Some("tool_use") => tool_calls.push(ToolCall {
                    id: item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    name: item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    arguments: item
                        .get("input")
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                }),
                _ => {}
            }
        }
        let finish_reason = body_val
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let assistant = Some(ChatMessage {
            role: "assistant".into(),
            content: if text.is_empty() { None } else { Some(text) },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            reasoning: if reasoning_parts.is_empty() {
                None
            } else {
                Some(reasoning_parts.join("\n\n"))
            },
            ..Default::default()
        });
        return ResponsePayload {
            assistant,
            finish_reason,
            usage,
        };
    }

    // OpenAI Responses API: `output` is an array of items keyed by `type`.
    // `reasoning` items carry a `summary[].text`; `message` items wrap a
    // `content[].text` payload.
    if let Some(output) = body_val.get("output").and_then(|v| v.as_array()) {
        let mut text = String::new();
        let mut reasoning_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        for item in output {
            match item.get("type").and_then(|v| v.as_str()) {
                Some("reasoning") => {
                    if let Some(arr) = item.get("summary").and_then(|v| v.as_array()) {
                        for s in arr {
                            if let Some(t) = s.get("text").and_then(|v| v.as_str()) {
                                reasoning_parts.push(t.to_string());
                            }
                        }
                    }
                }
                Some("message") => {
                    if let Some(arr) = item.get("content").and_then(|v| v.as_array()) {
                        for c in arr {
                            if let Some(t) = c.get("text").and_then(|v| v.as_str()) {
                                text.push_str(t);
                            }
                        }
                    }
                }
                Some("function_call") => tool_calls.push(ToolCall {
                    id: item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    name: item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    arguments: item
                        .get("arguments")
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default(),
                }),
                _ => {}
            }
        }
        if !text.is_empty() || !reasoning_parts.is_empty() || !tool_calls.is_empty() {
            let assistant = Some(ChatMessage {
                role: "assistant".into(),
                content: if text.is_empty() { None } else { Some(text) },
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
                reasoning: if reasoning_parts.is_empty() {
                    None
                } else {
                    Some(reasoning_parts.join("\n\n"))
                },
                ..Default::default()
            });
            let finish_reason = body_val
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            return ResponsePayload {
                assistant,
                finish_reason,
                usage,
            };
        }
    }

    // Gemini native shape: `candidates[0].content.parts[]` with `thought:true`
    // marking reasoning parts.
    if let Some(candidates) = body_val.get("candidates").and_then(|v| v.as_array()) {
        if let Some(first) = candidates.first() {
            let mut text = String::new();
            let mut reasoning_parts: Vec<String> = Vec::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            if let Some(parts) = first
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
            {
                for part in parts {
                    let is_thought = part
                        .get("thought")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                        if is_thought {
                            reasoning_parts.push(t.to_string());
                        } else {
                            text.push_str(t);
                        }
                    } else if let Some(fc) = part.get("functionCall") {
                        tool_calls.push(ToolCall {
                            id: String::new(),
                            name: fc
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            arguments: fc
                                .get("args")
                                .map(|v| v.to_string())
                                .unwrap_or_default(),
                        });
                    }
                }
            }
            if !text.is_empty() || !reasoning_parts.is_empty() || !tool_calls.is_empty() {
                let assistant = Some(ChatMessage {
                    role: "assistant".into(),
                    content: if text.is_empty() { None } else { Some(text) },
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    reasoning: if reasoning_parts.is_empty() {
                        None
                    } else {
                        Some(reasoning_parts.join("\n\n"))
                    },
                    ..Default::default()
                });
                let finish_reason = first
                    .get("finishReason")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                return ResponsePayload {
                    assistant,
                    finish_reason,
                    usage,
                };
            }
        }
    }

    ResponsePayload {
        assistant: None,
        finish_reason: None,
        usage,
    }
}

pub(crate) fn parse_message(v: &Value) -> ChatMessage {
    let role = v
        .get("role")
        .and_then(|x| x.as_str())
        .unwrap_or("user")
        .to_string();

    // Reasoning collected from sibling fields and (for Anthropic-shape content
    // arrays) from `thinking` blocks. Multiple sources concatenate.
    let mut reasoning_parts: Vec<String> = Vec::new();

    let content = match v.get("content") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(arr)) => {
            // Anthropic / multimodal: collect text parts; route `thinking` /
            // `redacted_thinking` blocks to reasoning instead of visible text.
            let mut s = String::new();
            for part in arr {
                let ty = part.get("type").and_then(|x| x.as_str());
                match ty {
                    Some("thinking") => {
                        if let Some(t) = part.get("thinking").and_then(|x| x.as_str()) {
                            reasoning_parts.push(t.to_string());
                        }
                    }
                    Some("redacted_thinking") => {
                        reasoning_parts.push("[redacted]".to_string());
                    }
                    _ => {
                        if let Some(t) = part.get("text").and_then(|x| x.as_str()) {
                            s.push_str(t);
                        } else if let Some(t) = part.as_str() {
                            s.push_str(t);
                        }
                    }
                }
            }
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }
        _ => None,
    };

    // Sibling fields used by Groq, OpenRouter, DeepSeek, xAI, NVIDIA NIM, Mistral magistral.
    if let Some(r) = v.get("reasoning").and_then(|x| x.as_str()) {
        if !r.is_empty() {
            reasoning_parts.push(r.to_string());
        }
    }
    if let Some(r) = v.get("reasoning_content").and_then(|x| x.as_str()) {
        if !r.is_empty() {
            reasoning_parts.push(r.to_string());
        }
    }

    // Inline `<think>...</think>` (common in local models). Strip from visible
    // content and append to reasoning.
    let content = content.map(|c| {
        let (visible, thought) = split_think_tags(&c);
        if !thought.is_empty() {
            reasoning_parts.push(thought);
        }
        visible
    });

    let reasoning = if reasoning_parts.is_empty() {
        None
    } else {
        Some(reasoning_parts.join("\n\n"))
    };

    let name = v
        .get("name")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let tool_call_id = v
        .get("tool_call_id")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());

    let tool_calls = v
        .get("tool_calls")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .map(|tc| {
                    let id = tc
                        .get("id")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let f = tc.get("function");
                    let name = f
                        .and_then(|x| x.get("name"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments = f
                        .and_then(|x| x.get("arguments"))
                        .map(|x| match x {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default();
                    ToolCall { id, name, arguments }
                })
                .collect()
        });

    // Legacy function_call
    let tool_calls = match tool_calls {
        Some(tc) => Some(tc),
        None => v.get("function_call").map(|f| {
            vec![ToolCall {
                id: String::new(),
                name: f
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                arguments: f
                    .get("arguments")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
            }]
        }),
    };

    ChatMessage {
        role,
        content,
        name,
        tool_call_id,
        tool_calls,
        reasoning,
    }
}

/// Extract `<think>...</think>` blocks (case-insensitive, byte-indexed so it
/// is UTF-8 safe). Returns `(visible, thought)` where `visible` has the tags
/// removed and `thought` is the concatenation of blocks joined by blank lines.
pub(crate) fn split_think_tags_public(s: &str) -> (String, String) {
    split_think_tags(s)
}

fn split_think_tags(s: &str) -> (String, String) {
    let lower = s.to_ascii_lowercase();
    if !lower.contains("<think>") {
        return (s.to_string(), String::new());
    }
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";

    let mut visible = String::with_capacity(s.len());
    let mut thoughts: Vec<String> = Vec::new();
    let mut cursor = 0usize;
    while cursor < s.len() {
        match lower[cursor..].find(OPEN) {
            Some(rel_open) => {
                let abs_open = cursor + rel_open;
                visible.push_str(&s[cursor..abs_open]);
                let after_open = abs_open + OPEN.len();
                match lower[after_open..].find(CLOSE) {
                    Some(rel_close) => {
                        let abs_close = after_open + rel_close;
                        let inner = s[after_open..abs_close].trim();
                        if !inner.is_empty() {
                            thoughts.push(inner.to_string());
                        }
                        cursor = abs_close + CLOSE.len();
                    }
                    None => {
                        let inner = s[after_open..].trim();
                        if !inner.is_empty() {
                            thoughts.push(inner.to_string());
                        }
                        cursor = s.len();
                    }
                }
            }
            None => {
                visible.push_str(&s[cursor..]);
                break;
            }
        }
    }
    (visible.trim().to_string(), thoughts.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_input_example() {
        let path = "../inputExample/audit.log";
        let content = fs::read_to_string(path).expect("fixture present");
        let out = parse_jsonl(&content);
        assert_eq!(out.events.len(), 9, "fixture has 9 events");
        assert!(out.warnings.is_empty(), "warnings: {:?}", out.warnings);
        let first = &out.events[0];
        assert_eq!(first.endpoint, "/v1/chat/completions");
        assert!(matches!(first.provider, Provider::Gemini));
        assert!(first
            .request
            .messages
            .iter()
            .any(|m| m.content.as_deref() == Some("Say hi in one short sentence.")));
    }

    #[test]
    fn parses_streamed_resp_body() {
        // Synthetic JSONL line whose resp_body is the SSE shape produced by
        // operatorlm's proxy logger: { data: "data: ...", size, truncated }.
        let sse = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\n\
                   data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2,\"total_tokens\":9}}\n\n\
                   data: [DONE]\n\n";
        let record = serde_json::json!({
            "id": "abc",
            "ts": "2026-05-15T00:00:00Z",
            "client": "127.0.0.1",
            "model": "myproxyllm",
            "status": 200,
            "duration_ms": 100,
            "req_body": { "model": "myproxyllm", "stream": true, "messages": [] },
            "resp_body": { "data": sse, "size": sse.len(), "truncated": false }
        });
        let line = serde_json::to_string(&record).unwrap();
        let out = parse_jsonl(&line);
        assert!(out.warnings.is_empty(), "warnings: {:?}", out.warnings);
        assert_eq!(out.events.len(), 1);
        let resp = &out.events[0].response;
        let assistant = resp.assistant.as_ref().expect("assistant present");
        assert_eq!(assistant.content.as_deref(), Some("Hello world"));
        assert_eq!(resp.finish_reason.as_deref(), Some("stop"));
        assert_eq!(resp.usage.completion_tokens, Some(2));
    }

    #[test]
    fn extracts_reasoning_field_groq_openrouter() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "42",
                    "reasoning": "Compute 6 * 7 = 42."
                },
                "finish_reason": "stop"
            }]
        });
        let resp = parse_response(Some(&body));
        let a = resp.assistant.unwrap();
        assert_eq!(a.content.as_deref(), Some("42"));
        assert_eq!(a.reasoning.as_deref(), Some("Compute 6 * 7 = 42."));
    }

    #[test]
    fn extracts_reasoning_content_deepseek_xai() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "ok",
                    "reasoning_content": "First I think, then I answer."
                }
            }]
        });
        let a = parse_response(Some(&body)).assistant.unwrap();
        assert_eq!(a.reasoning.as_deref(), Some("First I think, then I answer."));
    }

    #[test]
    fn extracts_anthropic_thinking_block() {
        let body = serde_json::json!({
            "content": [
                { "type": "thinking", "thinking": "Let me reason step by step." },
                { "type": "text", "text": "Hi!" }
            ],
            "stop_reason": "end_turn"
        });
        let resp = parse_response(Some(&body));
        let a = resp.assistant.unwrap();
        assert_eq!(a.content.as_deref(), Some("Hi!"));
        assert_eq!(a.reasoning.as_deref(), Some("Let me reason step by step."));
    }

    #[test]
    fn extracts_openai_responses_reasoning_summary() {
        let body = serde_json::json!({
            "output": [
                { "type": "reasoning",
                  "summary": [
                      { "type": "summary_text", "text": "Analyzed the prompt." },
                      { "type": "summary_text", "text": "Picked an approach." }
                  ] },
                { "type": "message",
                  "content": [ { "type": "output_text", "text": "Final answer." } ] }
            ],
            "status": "completed"
        });
        let a = parse_response(Some(&body)).assistant.unwrap();
        assert_eq!(a.content.as_deref(), Some("Final answer."));
        assert_eq!(
            a.reasoning.as_deref(),
            Some("Analyzed the prompt.\n\nPicked an approach.")
        );
    }

    #[test]
    fn extracts_gemini_thought_part() {
        let body = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "thought": true, "text": "I should add 2+2." },
                        { "text": "4" }
                    ]
                },
                "finishReason": "STOP"
            }]
        });
        let a = parse_response(Some(&body)).assistant.unwrap();
        assert_eq!(a.content.as_deref(), Some("4"));
        assert_eq!(a.reasoning.as_deref(), Some("I should add 2+2."));
    }

    #[test]
    fn strips_think_tags_into_reasoning() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "<think>plan carefully</think>The answer is 7."
                }
            }]
        });
        let a = parse_response(Some(&body)).assistant.unwrap();
        assert_eq!(a.content.as_deref(), Some("The answer is 7."));
        assert_eq!(a.reasoning.as_deref(), Some("plan carefully"));
    }

    #[test]
    fn split_think_tags_is_utf8_safe() {
        let (visible, thought) = split_think_tags("<think>razoná en español</think>ñandú");
        assert_eq!(visible, "ñandú");
        assert_eq!(thought, "razoná en español");
    }

    #[test]
    fn assembles_streamed_reasoning_delta() {
        let sse = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"reasoning\":\"think \"},\"finish_reason\":null}]}\n\n\
                   data: {\"choices\":[{\"delta\":{\"reasoning\":\"more.\"},\"finish_reason\":null}]}\n\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}]}\n\n\
                   data: [DONE]\n\n";
        let record = serde_json::json!({
            "id": "r1",
            "ts": "2026-05-15T00:00:00Z",
            "client": "127.0.0.1",
            "model": "groq/qwen-reasoning",
            "status": 200,
            "duration_ms": 50,
            "req_body": { "model": "qwen", "stream": true, "messages": [] },
            "resp_body": { "data": sse, "size": sse.len(), "truncated": false }
        });
        let out = parse_jsonl(&serde_json::to_string(&record).unwrap());
        let a = out.events[0].response.assistant.as_ref().unwrap();
        assert_eq!(a.content.as_deref(), Some("done"));
        assert_eq!(a.reasoning.as_deref(), Some("think more."));
    }

    #[test]
    fn marks_truncated_stream_when_no_done() {
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n";
        let record = serde_json::json!({
            "id": "abc",
            "ts": "2026-05-15T00:00:00Z",
            "client": "127.0.0.1",
            "model": "myproxyllm",
            "status": 200,
            "duration_ms": 100,
            "req_body": { "model": "myproxyllm", "stream": true, "messages": [] },
            "resp_body": { "data": sse, "size": sse.len(), "truncated": true }
        });
        let line = serde_json::to_string(&record).unwrap();
        let out = parse_jsonl(&line);
        let resp = &out.events[0].response;
        assert_eq!(resp.finish_reason.as_deref(), Some("truncated"));
        assert_eq!(
            resp.assistant.as_ref().unwrap().content.as_deref(),
            Some("partial")
        );
    }
}
