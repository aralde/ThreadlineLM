use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::model::{
    ChatMessage, LogEvent, Provider, RequestPayload, ResponsePayload, ToolCall, Upstream, Usage,
};

use super::ParseOutcome;

/// Top-level entry: dispatch between bare-span JSONL, JSON arrays, and OTLP/JSON
/// envelopes (`resourceSpans` wrapper).
pub fn parse(content: &str) -> ParseOutcome {
    let mut events = Vec::new();
    let mut warnings = Vec::new();
    let trimmed = content.trim_start();

    // Single OTLP envelope (whole file is one JSON object).
    if trimmed.starts_with('{')
        && (trimmed.contains("\"resourceSpans\"") && !looks_like_jsonl(content))
    {
        match serde_json::from_str::<Value>(content) {
            Ok(root) => collect_from_otlp(&root, &mut events, &mut warnings),
            Err(e) => warnings.push(format!("invalid json: {}", e)),
        }
        return ParseOutcome { events, warnings };
    }

    // JSON array of spans.
    if trimmed.starts_with('[') {
        match serde_json::from_str::<Value>(content) {
            Ok(Value::Array(arr)) => {
                for (idx, span) in arr.iter().enumerate() {
                    process_value(span, idx, &mut events, &mut warnings);
                }
            }
            Ok(_) => warnings.push("expected a JSON array".into()),
            Err(e) => warnings.push(format!("invalid json: {}", e)),
        }
        return ParseOutcome { events, warnings };
    }

    // JSONL: one object per line.
    for (idx, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(v) => process_value(&v, idx, &mut events, &mut warnings),
            Err(e) => warnings.push(format!("line {}: invalid json: {}", idx + 1, e)),
        }
    }

    ParseOutcome { events, warnings }
}

/// Heuristic: a JSONL stream has more than one self-contained `{...}` line.
fn looks_like_jsonl(content: &str) -> bool {
    content.lines().filter(|l| l.trim().starts_with('{')).count() > 1
}

fn process_value(v: &Value, idx: usize, events: &mut Vec<LogEvent>, warnings: &mut Vec<String>) {
    // An OTLP/JSON line wraps spans in resourceSpans → scopeSpans → spans.
    if v.get("resourceSpans").is_some() {
        collect_from_otlp(v, events, warnings);
        return;
    }
    match span_to_event(v) {
        Ok(Some(e)) => events.push(e),
        Ok(None) => { /* not a gen_ai span — silently skip */ }
        Err(err) => warnings.push(format!("line {}: {}", idx + 1, err)),
    }
}

fn collect_from_otlp(root: &Value, events: &mut Vec<LogEvent>, warnings: &mut Vec<String>) {
    let Some(rs) = root.get("resourceSpans").and_then(|v| v.as_array()) else {
        return;
    };
    for r in rs {
        let Some(ss) = r.get("scopeSpans").and_then(|v| v.as_array()) else {
            continue;
        };
        for s in ss {
            let Some(spans) = s.get("spans").and_then(|v| v.as_array()) else {
                continue;
            };
            for span in spans {
                match span_to_event(span) {
                    Ok(Some(e)) => events.push(e),
                    Ok(None) => {}
                    Err(err) => warnings.push(err),
                }
            }
        }
    }
}

fn span_to_event(span: &Value) -> Result<Option<LogEvent>, String> {
    // Build an attribute lookup that works for both shapes:
    //  1. Plain object: { "gen_ai.system": "openai", ... }
    //  2. OTLP KeyValue array: [{ "key": "gen_ai.system", "value": { "stringValue": "openai" }}, ...]
    let attrs = AttrMap::from(span.get("attributes"));

    // Only emit events for spans that carry gen_ai.* attributes.
    if !attrs.has_gen_ai() {
        return Ok(None);
    }

    let span_id = span
        .get("spanId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".into());
    let trace_id = span
        .get("traceId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let id = if trace_id.is_empty() {
        span_id.clone()
    } else {
        format!("{}:{}", &trace_id[..trace_id.len().min(8)], span_id)
    };

    let start_ns = read_unix_nano(span.get("startTimeUnixNano"));
    let end_ns = read_unix_nano(span.get("endTimeUnixNano"));
    let ts = start_ns.and_then(nanos_to_rfc3339).unwrap_or_else(|| {
        Utc::now().to_rfc3339() // best-effort: missing timestamps are very rare in valid exports
    });
    let duration_ms = match (start_ns, end_ns) {
        (Some(s), Some(e)) if e >= s => (e - s) / 1_000_000,
        _ => 0,
    };

    let system = attrs.get_str("gen_ai.system").unwrap_or_default();
    let provider = Provider::from_otel_system(&system);
    let model = attrs
        .get_str("gen_ai.request.model")
        .or_else(|| attrs.get_str("gen_ai.response.model"))
        .unwrap_or_else(|| "unknown".into());

    let endpoint = endpoint_for_op(&attrs.get_str("gen_ai.operation.name").unwrap_or_default());

    let server_addr = attrs.get_str("server.address");
    let server_status = attrs.get_u64("http.response.status_code").map(|x| x as u16);
    let upstream = server_addr.map(|url| Upstream {
        url,
        status: server_status,
    });

    let status_code = attrs
        .get_u64("http.response.status_code")
        .map(|x| x as u16)
        .unwrap_or_else(|| {
            let code = span
                .get("status")
                .and_then(|s| s.get("code"))
                .and_then(|c| c.as_str().or_else(|| None))
                .unwrap_or("");
            if code.eq_ignore_ascii_case("ERROR") {
                500
            } else {
                200
            }
        });

    // Walk events for input messages + the model's reply.
    let mut messages: Vec<ChatMessage> = Vec::new();
    let mut assistant: Option<ChatMessage> = None;
    if let Some(evts) = span.get("events").and_then(|v| v.as_array()) {
        for ev in evts {
            let name = ev.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let ev_attrs = AttrMap::from(ev.get("attributes"));
            match name {
                "gen_ai.system.message" => messages.push(message_from_event("system", &ev_attrs)),
                "gen_ai.user.message" => messages.push(message_from_event("user", &ev_attrs)),
                "gen_ai.tool.message" => messages.push(message_from_event("tool", &ev_attrs)),
                "gen_ai.assistant.message" => {
                    // The LAST assistant.message is the response; earlier ones (if any)
                    // are part of the input history.
                    if let Some(prev) = assistant.take() {
                        messages.push(prev);
                    }
                    assistant = Some(message_from_event("assistant", &ev_attrs));
                }
                "gen_ai.choice" => {
                    // Newer semconv: { index, finish_reason, message: {...} }.
                    if let Some(msg) = ev_attrs.get_object("message") {
                        assistant = Some(super::openai_audit::parse_message(&msg));
                    } else {
                        assistant = Some(message_from_event("assistant", &ev_attrs));
                    }
                }
                _ => {}
            }
        }
    }

    let finish_reason = attrs
        .get_array_first_str("gen_ai.response.finish_reasons")
        .or_else(|| attrs.get_str("gen_ai.response.finish_reason"));

    let usage = Usage {
        prompt_tokens: attrs.get_u64("gen_ai.usage.input_tokens"),
        completion_tokens: attrs.get_u64("gen_ai.usage.output_tokens"),
        total_tokens: match (
            attrs.get_u64("gen_ai.usage.input_tokens"),
            attrs.get_u64("gen_ai.usage.output_tokens"),
        ) {
            (Some(p), Some(c)) => Some(p + c),
            _ => None,
        },
    };

    let request = RequestPayload {
        model: model.clone(),
        messages,
        stream: attrs.get_bool("gen_ai.request.is_streaming").unwrap_or(false),
        tools_declared: attrs.get_u64("gen_ai.request.tools.count").unwrap_or(0) as u32,
    };
    let response = ResponsePayload {
        assistant,
        finish_reason,
        usage,
    };

    Ok(Some(LogEvent {
        id,
        ts,
        client: attrs
            .get_str("client.address")
            .unwrap_or_else(|| "otel".into()),
        ua: attrs.get_str("user_agent.original"),
        provider,
        model,
        endpoint,
        request,
        response,
        upstream,
        status: status_code,
        duration_ms,
        raw: span.clone(),
    }))
}

fn message_from_event(role: &str, attrs: &AttrMap) -> ChatMessage {
    let content = attrs.get_str("content");
    let tool_call_id = attrs.get_str("id").filter(|_| role == "tool");
    let name = attrs.get_str("name");

    // `tool_calls` may be a JSON-encoded string OR an inline JSON array.
    let tool_calls = attrs.get_value("tool_calls").and_then(|v| {
        let arr = match &v {
            Value::String(s) => serde_json::from_str::<Value>(s).ok(),
            other => Some(other.clone()),
        };
        arr.as_ref()
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .map(|tc| ToolCall {
                        id: tc.get("id").and_then(|x| x.as_str()).unwrap_or("").into(),
                        name: tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .or_else(|| tc.get("name"))
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .into(),
                        arguments: tc
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .or_else(|| tc.get("arguments"))
                            .map(|x| match x {
                                Value::String(s) => s.clone(),
                                other => other.to_string(),
                            })
                            .unwrap_or_default(),
                    })
                    .collect::<Vec<_>>()
            })
    });

    ChatMessage {
        role: role.into(),
        content,
        name,
        tool_call_id,
        tool_calls,
    }
}

fn endpoint_for_op(op: &str) -> String {
    match op {
        "chat" => "/v1/chat/completions",
        "text_completion" => "/v1/completions",
        "embeddings" => "/v1/embeddings",
        "" => "/v1/chat/completions",
        other => other,
    }
    .to_string()
}

fn read_unix_nano(v: Option<&Value>) -> Option<u64> {
    let v = v?;
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    // OTLP/JSON serializes uint64 as a decimal string.
    v.as_str().and_then(|s| s.parse::<u64>().ok())
}

fn nanos_to_rfc3339(nanos: u64) -> Option<String> {
    let secs = (nanos / 1_000_000_000) as i64;
    let nsec = (nanos % 1_000_000_000) as u32;
    DateTime::<Utc>::from_timestamp(secs, nsec).map(|dt| dt.to_rfc3339())
}

// --- Attribute lookup that abstracts over OTLP-array and plain-object shapes ---

struct AttrMap<'a> {
    obj: Option<&'a serde_json::Map<String, Value>>,
    arr: Option<&'a Vec<Value>>,
}

impl<'a> AttrMap<'a> {
    fn from(v: Option<&'a Value>) -> Self {
        match v {
            Some(Value::Object(o)) => AttrMap {
                obj: Some(o),
                arr: None,
            },
            Some(Value::Array(a)) => AttrMap {
                obj: None,
                arr: Some(a),
            },
            _ => AttrMap {
                obj: None,
                arr: None,
            },
        }
    }

    fn has_gen_ai(&self) -> bool {
        if let Some(o) = self.obj {
            return o.keys().any(|k| k.starts_with("gen_ai."));
        }
        if let Some(a) = self.arr {
            return a.iter().any(|kv| {
                kv.get("key")
                    .and_then(|v| v.as_str())
                    .map(|k| k.starts_with("gen_ai."))
                    .unwrap_or(false)
            });
        }
        false
    }

    fn get_value(&self, key: &str) -> Option<Value> {
        if let Some(o) = self.obj {
            return o.get(key).cloned();
        }
        if let Some(a) = self.arr {
            for kv in a {
                if kv.get("key").and_then(|v| v.as_str()) == Some(key) {
                    return kv.get("value").map(unwrap_otlp_value);
                }
            }
        }
        None
    }

    fn get_str(&self, key: &str) -> Option<String> {
        match self.get_value(key)? {
            Value::String(s) => Some(s),
            other => Some(other.to_string()),
        }
    }

    fn get_u64(&self, key: &str) -> Option<u64> {
        match self.get_value(key)? {
            Value::Number(n) => n.as_u64(),
            Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }

    fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get_value(key)? {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }

    fn get_object(&self, key: &str) -> Option<Value> {
        let v = self.get_value(key)?;
        match v {
            Value::Object(_) => Some(v),
            Value::String(s) => serde_json::from_str(&s).ok(),
            _ => None,
        }
    }

    fn get_array_first_str(&self, key: &str) -> Option<String> {
        let v = self.get_value(key)?;
        let arr = match v {
            Value::Array(a) => a,
            Value::String(s) => serde_json::from_str::<Vec<Value>>(&s).ok()?,
            _ => return None,
        };
        arr.first().and_then(|x| x.as_str()).map(|s| s.to_string())
    }
}

/// OTLP/JSON wraps every attribute value: `{ "stringValue": "x" }`,
/// `{ "intValue": "42" }`, `{ "arrayValue": { "values": [...] } }`, etc.
/// Unwrap to the bare JSON value.
fn unwrap_otlp_value(v: &Value) -> Value {
    if let Some(o) = v.as_object() {
        if let Some(s) = o.get("stringValue") {
            return s.clone();
        }
        if let Some(b) = o.get("boolValue") {
            return b.clone();
        }
        if let Some(i) = o.get("intValue") {
            // OTLP encodes int64 as a string.
            if let Some(s) = i.as_str() {
                if let Ok(n) = s.parse::<i64>() {
                    return Value::from(n);
                }
            }
            return i.clone();
        }
        if let Some(d) = o.get("doubleValue") {
            return d.clone();
        }
        if let Some(arr) = o.get("arrayValue").and_then(|a| a.get("values")) {
            if let Some(a) = arr.as_array() {
                return Value::Array(a.iter().map(unwrap_otlp_value).collect());
            }
        }
    }
    v.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_attrs_span_with_chat() {
        let span = serde_json::json!({
            "spanId": "ab12",
            "traceId": "trace-0000",
            "name": "chat gpt-4",
            "startTimeUnixNano": "1700000000000000000",
            "endTimeUnixNano":   "1700000001416000000",
            "attributes": {
                "gen_ai.system": "openai",
                "gen_ai.operation.name": "chat",
                "gen_ai.request.model": "gpt-4",
                "gen_ai.response.finish_reasons": ["stop"],
                "gen_ai.usage.input_tokens": 17,
                "gen_ai.usage.output_tokens": 3,
                "server.address": "api.openai.com",
                "http.response.status_code": 200
            },
            "events": [
                { "name": "gen_ai.user.message",
                  "attributes": { "content": "Say hi." } },
                { "name": "gen_ai.assistant.message",
                  "attributes": { "content": "Hi!" } }
            ],
            "status": { "code": "OK" }
        });
        let jsonl = serde_json::to_string(&span).unwrap();
        let out = parse(&jsonl);
        assert!(out.warnings.is_empty(), "warnings: {:?}", out.warnings);
        assert_eq!(out.events.len(), 1);
        let e = &out.events[0];
        assert!(matches!(e.provider, Provider::OpenAI));
        assert_eq!(e.model, "gpt-4");
        assert_eq!(e.endpoint, "/v1/chat/completions");
        assert_eq!(e.duration_ms, 1416);
        assert_eq!(e.request.messages[0].content.as_deref(), Some("Say hi."));
        let a = e.response.assistant.as_ref().unwrap();
        assert_eq!(a.role, "assistant");
        assert_eq!(a.content.as_deref(), Some("Hi!"));
        assert_eq!(e.response.finish_reason.as_deref(), Some("stop"));
        assert_eq!(e.response.usage.completion_tokens, Some(3));
        assert_eq!(e.response.usage.total_tokens, Some(20));
    }

    #[test]
    fn parses_otlp_kv_attrs_with_tool_calls() {
        // OTLP/JSON wire shape: attributes as KeyValue array, values wrapped.
        let span = serde_json::json!({
            "spanId": "cd34",
            "traceId": "trace-anth",
            "name": "chat",
            "startTimeUnixNano": 1700000000000000000u64,
            "endTimeUnixNano":   1700000000500000000u64,
            "attributes": [
                { "key": "gen_ai.system", "value": { "stringValue": "anthropic" } },
                { "key": "gen_ai.request.model", "value": { "stringValue": "claude-3-opus" } },
                { "key": "gen_ai.usage.input_tokens", "value": { "intValue": "42" } },
                { "key": "gen_ai.usage.output_tokens", "value": { "intValue": "7" } }
            ],
            "events": [
                { "name": "gen_ai.user.message",
                  "attributes": [
                      { "key": "content", "value": { "stringValue": "List files." } }
                  ]
                },
                { "name": "gen_ai.assistant.message",
                  "attributes": [
                      { "key": "tool_calls", "value": { "stringValue":
                        "[{\"id\":\"call_1\",\"function\":{\"name\":\"ls\",\"arguments\":\"{}\"}}]"
                      } }
                  ]
                }
            ],
            "status": { "code": "OK" }
        });
        let out = parse(&serde_json::to_string(&span).unwrap());
        assert!(out.warnings.is_empty(), "warnings: {:?}", out.warnings);
        let e = &out.events[0];
        assert!(matches!(e.provider, Provider::Anthropic));
        assert_eq!(e.model, "claude-3-opus");
        assert_eq!(e.response.usage.prompt_tokens, Some(42));
        let tcs = e
            .response
            .assistant
            .as_ref()
            .and_then(|a| a.tool_calls.as_ref())
            .expect("tool_calls present");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_1");
        assert_eq!(tcs[0].name, "ls");
        assert_eq!(tcs[0].arguments, "{}");
    }

    #[test]
    fn skips_non_genai_spans() {
        let span = serde_json::json!({
            "spanId": "ef56",
            "name": "GET /healthz",
            "attributes": { "http.method": "GET" }
        });
        let out = parse(&serde_json::to_string(&span).unwrap());
        assert_eq!(out.events.len(), 0);
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn unwraps_otlp_envelope() {
        let envelope = serde_json::json!({
            "resourceSpans": [{
                "scopeSpans": [{
                    "spans": [{
                        "spanId": "x",
                        "startTimeUnixNano": "1700000000000000000",
                        "endTimeUnixNano":   "1700000000100000000",
                        "attributes": { "gen_ai.system": "groq", "gen_ai.request.model": "llama-3" },
                        "events": []
                    }]
                }]
            }]
        });
        let out = parse(&serde_json::to_string(&envelope).unwrap());
        assert_eq!(out.events.len(), 1);
        assert!(matches!(out.events[0].provider, Provider::Groq));
    }
}
