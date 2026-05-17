use chrono::DateTime;
use serde_json::Value;

use crate::model::{LogEvent, Provider, RequestPayload, ResponsePayload, Upstream, Usage};

use super::openai_audit::{parse_message, parse_response};
use super::{parse_jsonl_or_array, ParseOutcome};

/// Parse LiteLLM "StandardLoggingPayload" records. Accepts both JSONL and
/// JSON-array wrappers.
pub fn parse(content: &str) -> ParseOutcome {
    parse_jsonl_or_array(content, record_to_event)
}

fn record_to_event(v: &Value) -> Result<LogEvent, String> {
    let id = v
        .get("id")
        .or_else(|| v.get("litellm_call_id"))
        .or_else(|| v.get("trace_id"))
        .and_then(|x| x.as_str())
        .ok_or("missing id / litellm_call_id / trace_id")?
        .to_string();

    let ts = v
        .get("startTime")
        .or_else(|| v.get("start_time"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or("missing startTime")?;

    let end_ts = v
        .get("endTime")
        .or_else(|| v.get("end_time"))
        .and_then(|x| x.as_str());

    let duration_ms = match (DateTime::parse_from_rfc3339(&ts).ok(), end_ts) {
        (Some(start), Some(end_s)) => DateTime::parse_from_rfc3339(end_s)
            .ok()
            .map(|end| (end.timestamp_millis() - start.timestamp_millis()).max(0) as u64)
            .unwrap_or(0),
        _ => v
            .get("response_ms")
            .and_then(|x| x.as_u64())
            .unwrap_or(0),
    };

    let model = v
        .get("model")
        .and_then(|x| x.as_str())
        .unwrap_or("unknown")
        .to_string();

    let provider_str = v
        .get("custom_llm_provider")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let provider = Provider::from_prefix(provider_str);

    let endpoint = endpoint_for_call_type(
        v.get("call_type").and_then(|x| x.as_str()).unwrap_or(""),
    );

    let api_base = v.get("api_base").and_then(|x| x.as_str()).map(String::from);
    let success = v
        .get("status")
        .and_then(|x| x.as_str())
        .map(|s| s.eq_ignore_ascii_case("success"))
        .unwrap_or(true);
    let status: u16 = if success { 200 } else { 500 };
    let upstream = api_base.map(|url| Upstream {
        url,
        status: Some(status),
    });

    let messages = v
        .get("messages")
        .and_then(|x| x.as_array())
        .map(|arr| arr.iter().map(parse_message).collect())
        .unwrap_or_default();
    let stream = v
        .get("stream")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let tools_declared = v
        .get("model_parameters")
        .and_then(|p| p.get("tools"))
        .and_then(|t| t.as_array())
        .map(|a| a.len() as u32)
        .unwrap_or(0);

    let request = RequestPayload {
        model: model.clone(),
        messages,
        stream,
        tools_declared,
    };

    // `response` may be an OpenAI-shape object, an Anthropic-shape object, or
    // a raw SSE string when the call was streamed — parse_response handles
    // all three.
    let response: ResponsePayload = parse_response(v.get("response"));

    // Fill usage from the top-level `usage` if the response object didn't carry it.
    let response = if response.usage.total_tokens.is_some()
        || response.usage.prompt_tokens.is_some()
        || response.usage.completion_tokens.is_some()
    {
        response
    } else if let Some(u) = v.get("usage") {
        ResponsePayload {
            usage: Usage {
                prompt_tokens: u.get("prompt_tokens").and_then(|x| x.as_u64()),
                completion_tokens: u.get("completion_tokens").and_then(|x| x.as_u64()),
                total_tokens: u.get("total_tokens").and_then(|x| x.as_u64()),
            },
            ..response
        }
    } else {
        response
    };

    let client = v
        .get("requester_ip_address")
        .and_then(|x| x.as_str())
        .map(String::from)
        .unwrap_or_else(|| "litellm".into());

    let ua = v
        .get("metadata")
        .and_then(|m| m.get("user_api_key_alias"))
        .and_then(|x| x.as_str())
        .map(String::from);

    Ok(LogEvent {
        id,
        ts,
        client,
        ua,
        provider,
        model,
        endpoint,
        request,
        response,
        upstream,
        status,
        duration_ms,
        raw: v.clone(),
    })
}

fn endpoint_for_call_type(call_type: &str) -> String {
    match call_type {
        "completion" | "acompletion" | "chat_completion" => "/v1/chat/completions",
        "text_completion" | "atext_completion" => "/v1/completions",
        "embedding" | "aembedding" => "/v1/embeddings",
        "image_generation" | "aimage_generation" => "/v1/images/generations",
        "" => "/v1/chat/completions",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_logging_payload() {
        let rec = serde_json::json!({
            "id": "chatcmpl-abc",
            "call_type": "completion",
            "api_base": "https://api.openai.com",
            "model": "gpt-4",
            "custom_llm_provider": "openai",
            "startTime": "2026-05-17T10:00:00Z",
            "endTime":   "2026-05-17T10:00:01.500Z",
            "stream": false,
            "messages": [{ "role": "user", "content": "Say hi." }],
            "response": {
                "id": "chatcmpl-abc",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "Hi!" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 5, "completion_tokens": 1, "total_tokens": 6 }
            },
            "status": "success"
        });
        let out = parse(&serde_json::to_string(&rec).unwrap());
        assert!(out.warnings.is_empty(), "warnings: {:?}", out.warnings);
        assert_eq!(out.events.len(), 1);
        let e = &out.events[0];
        assert!(matches!(e.provider, Provider::OpenAI));
        assert_eq!(e.model, "gpt-4");
        assert_eq!(e.endpoint, "/v1/chat/completions");
        assert_eq!(e.duration_ms, 1500);
        assert_eq!(e.status, 200);
        assert_eq!(e.request.messages[0].content.as_deref(), Some("Say hi."));
        let a = e.response.assistant.as_ref().unwrap();
        assert_eq!(a.content.as_deref(), Some("Hi!"));
        assert_eq!(e.response.finish_reason.as_deref(), Some("stop"));
        assert_eq!(e.response.usage.total_tokens, Some(6));
    }

    #[test]
    fn assembles_streamed_response() {
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\"!\"},\"finish_reason\":\"stop\"}]}\n\n\
                   data: [DONE]\n\n";
        let rec = serde_json::json!({
            "id": "stream-1",
            "call_type": "acompletion",
            "api_base": "https://api.groq.com",
            "model": "llama-3",
            "custom_llm_provider": "groq",
            "startTime": "2026-05-17T10:00:00Z",
            "endTime":   "2026-05-17T10:00:00.200Z",
            "stream": true,
            "messages": [{ "role": "user", "content": "Hola" }],
            "response": { "data": sse, "size": sse.len(), "truncated": false },
            "status": "success",
            "usage": { "prompt_tokens": 4, "completion_tokens": 2, "total_tokens": 6 }
        });
        let out = parse(&serde_json::to_string(&rec).unwrap());
        assert!(out.warnings.is_empty(), "warnings: {:?}", out.warnings);
        let e = &out.events[0];
        assert!(matches!(e.provider, Provider::Groq));
        assert!(e.request.stream);
        let a = e.response.assistant.as_ref().expect("assistant present");
        assert_eq!(a.content.as_deref(), Some("Hi!"));
        assert_eq!(e.response.finish_reason.as_deref(), Some("stop"));
        // SSE didn't carry usage, so the top-level `usage` should have backfilled it.
        assert_eq!(e.response.usage.total_tokens, Some(6));
    }

    #[test]
    fn marks_failure_as_status_500() {
        let rec = serde_json::json!({
            "id": "err-1",
            "call_type": "completion",
            "api_base": "https://api.openai.com",
            "model": "gpt-4",
            "custom_llm_provider": "openai",
            "startTime": "2026-05-17T10:00:00Z",
            "endTime":   "2026-05-17T10:00:00.050Z",
            "messages": [],
            "status": "failure"
        });
        let out = parse(&serde_json::to_string(&rec).unwrap());
        assert_eq!(out.events[0].status, 500);
    }
}
