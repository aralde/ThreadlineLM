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
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        for item in content {
            match item.get("type").and_then(|v| v.as_str()) {
                Some("text") => {
                    if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                        text.push_str(t);
                    }
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
            ..Default::default()
        });
        return ResponsePayload {
            assistant,
            finish_reason,
            usage,
        };
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

    let content = match v.get("content") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(arr)) => {
            // Anthropic / multimodal: collect text parts
            let mut s = String::new();
            for part in arr {
                if let Some(t) = part.get("text").and_then(|x| x.as_str()) {
                    s.push_str(t);
                } else if let Some(t) = part.as_str() {
                    s.push_str(t);
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
    }
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
