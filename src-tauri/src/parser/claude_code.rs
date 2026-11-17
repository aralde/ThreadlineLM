use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::model::{
    ChatMessage, LogEvent, Provider, RequestPayload, ResponsePayload, ToolCall, Usage,
};

use super::ParseOutcome;

pub const ENDPOINT: &str = "claude-code/transcript";

pub fn is_transcript_event(e: &LogEvent) -> bool {
    e.endpoint == ENDPOINT
}

/// True when the JSON object looks like a Claude Code transcript entry.
pub fn looks_like_entry(v: &Value) -> bool {
    v.get("sessionId").map_or(false, |s| s.is_string())
        && v.get("uuid").map_or(false, |s| s.is_string())
        && v.get("parentUuid").is_some()
        && v.get("type").map_or(false, |s| s.is_string())
}

/// Subagent transcripts that sit next to a session file:
/// `<dir>/<session-id>/subagents/agent-*.jsonl`.
pub fn companion_paths(session_file: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (session_file.parent(), session_file.file_stem()) else {
        return Vec::new();
    };
    let sub = dir.join(stem).join("subagents");
    let Ok(rd) = std::fs::read_dir(&sub) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |x| x == "jsonl"))
        .collect();
    out.sort();
    out
}

struct Pending {
    uuid: String,
    ts: String,
    session_id: String,
    agent_id: Option<String>,
    message_id: String,
    model: String,
    content: Vec<String>,
    reasoning: Vec<String>,
    tool_calls: Vec<ToolCall>,
    usage: Option<Value>,
    stop_reason: Option<String>,
    entries: Vec<Value>,
}

pub fn parse(content: &str) -> ParseOutcome {
    let mut events = Vec::new();
    let mut warnings = Vec::new();

    let mut delta: Vec<ChatMessage> = Vec::new();
    let mut delta_entries: Vec<Value> = Vec::new();
    let mut links: Map<String, Value> = Map::new();
    let mut pending: Option<Pending> = None;
    let mut last: Option<(String, String, String, Option<String>)> = None;

    for (idx, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("line {}: invalid json: {}", idx + 1, e));
                continue;
            }
        };
        let kind = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let str_of = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);

        if kind == "progress" {
            let d = v.get("data");
            if let (Some(agent), Some(parent)) = (
                d.and_then(|d| d.get("agentId")).and_then(|x| x.as_str()),
                str_of("parentToolUseID"),
            ) {
                links.entry(agent.to_string()).or_insert(Value::String(parent));
            }
            continue;
        }
        if kind != "user" && kind != "assistant" {
            continue;
        }
        let Some(msg) = v.get("message") else {
            warnings.push(format!("line {}: missing message", idx + 1));
            continue;
        };
        let (Some(uuid), Some(ts), Some(session_id)) =
            (str_of("uuid"), str_of("timestamp"), str_of("sessionId"))
        else {
            warnings.push(format!("line {}: missing uuid / timestamp / sessionId", idx + 1));
            continue;
        };
        let agent_id = str_of("agentId");
        last = Some((uuid.clone(), ts.clone(), session_id.clone(), agent_id.clone()));

        if kind == "assistant" {
            let mid = msg
                .get("id")
                .and_then(|x| x.as_str())
                .map(String::from)
                .unwrap_or_else(|| uuid.clone());
            if pending.as_ref().map_or(false, |p| p.message_id != mid) {
                flush(&mut pending, &mut delta, &mut delta_entries, &mut links, &mut events);
            }
            let p = pending.get_or_insert_with(|| Pending {
                uuid: uuid.clone(),
                ts: ts.clone(),
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                message_id: mid,
                model: String::new(),
                content: Vec::new(),
                reasoning: Vec::new(),
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
                entries: Vec::new(),
            });
            if let Some(m) = msg.get("model").and_then(|x| x.as_str()) {
                p.model = m.to_string();
            }
            if let Some(u) = msg.get("usage") {
                p.usage = Some(u.clone());
            }
            if let Some(s) = msg.get("stop_reason").and_then(|x| x.as_str()) {
                p.stop_reason = Some(s.to_string());
            }
            match msg.get("content") {
                Some(Value::String(s)) => p.content.push(s.clone()),
                Some(Value::Array(blocks)) => {
                    for b in blocks {
                        match b.get("type").and_then(|x| x.as_str()) {
                            Some("text") => {
                                if let Some(t) = b.get("text").and_then(|x| x.as_str()) {
                                    p.content.push(t.to_string());
                                }
                            }
                            Some("thinking") => {
                                if let Some(t) = b.get("thinking").and_then(|x| x.as_str()) {
                                    if !t.is_empty() {
                                        p.reasoning.push(t.to_string());
                                    }
                                }
                            }
                            Some("tool_use") => p.tool_calls.push(ToolCall {
                                id: b.get("id").and_then(|x| x.as_str()).unwrap_or("").into(),
                                name: b.get("name").and_then(|x| x.as_str()).unwrap_or("").into(),
                                arguments: b
                                    .get("input")
                                    .map(|i| i.to_string())
                                    .unwrap_or_else(|| "{}".into()),
                            }),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            p.entries.push(v.clone());
        } else {
            flush(&mut pending, &mut delta, &mut delta_entries, &mut links, &mut events);
            let agent = v
                .get("toolUseResult")
                .and_then(|r| r.get("agentId"))
                .and_then(|x| x.as_str());
            if let Some(agent) = agent {
                if let Some(tid) = first_tool_result_id(msg) {
                    links.insert(agent.to_string(), Value::String(tid));
                }
            }
            user_messages(msg, &mut delta);
            delta_entries.push(v.clone());
        }
    }
    flush(&mut pending, &mut delta, &mut delta_entries, &mut links, &mut events);

    if !delta.is_empty() {
        if let Some((uuid, ts, session_id, agent_id)) = last {
            events.push(make_event(
                uuid,
                ts,
                session_id,
                agent_id,
                "unknown".into(),
                std::mem::take(&mut delta),
                None,
                Usage::default(),
                None,
                std::mem::take(&mut delta_entries),
                std::mem::take(&mut links),
            ));
        }
    }

    ParseOutcome { events, warnings }
}

fn flush(
    pending: &mut Option<Pending>,
    delta: &mut Vec<ChatMessage>,
    delta_entries: &mut Vec<Value>,
    links: &mut Map<String, Value>,
    events: &mut Vec<LogEvent>,
) {
    let Some(p) = pending.take() else { return };
    let text = p.content.join("\n");
    let assistant = ChatMessage {
        role: "assistant".into(),
        content: if text.is_empty() { None } else { Some(text) },
        tool_calls: if p.tool_calls.is_empty() { None } else { Some(p.tool_calls) },
        reasoning: if p.reasoning.is_empty() { None } else { Some(p.reasoning.join("\n")) },
        ..Default::default()
    };
    let mut entries = std::mem::take(delta_entries);
    entries.extend(p.entries);
    events.push(make_event(
        p.uuid,
        p.ts,
        p.session_id,
        p.agent_id,
        if p.model.is_empty() { "unknown".into() } else { p.model },
        std::mem::take(delta),
        Some(assistant),
        usage_of(p.usage.as_ref()),
        p.stop_reason,
        entries,
        std::mem::take(links),
    ));
}

#[allow(clippy::too_many_arguments)]
fn make_event(
    id: String,
    ts: String,
    session_id: String,
    agent_id: Option<String>,
    model: String,
    messages: Vec<ChatMessage>,
    assistant: Option<ChatMessage>,
    usage: Usage,
    finish_reason: Option<String>,
    entries: Vec<Value>,
    links: Map<String, Value>,
) -> LogEvent {
    let raw = json!({
        "agent_id": agent_id,
        "agent_links": links,
        "entries": entries,
    });
    LogEvent {
        id,
        ts,
        client: session_id,
        ua: agent_id.map(|a| format!("subagent {a}")),
        provider: Provider::Anthropic,
        model: model.clone(),
        endpoint: ENDPOINT.into(),
        request: RequestPayload {
            model,
            messages,
            stream: false,
            tools_declared: 0,
        },
        response: ResponsePayload {
            assistant,
            finish_reason,
            usage,
        },
        upstream: None,
        status: 0,
        duration_ms: 0,
        raw,
    }
}

fn usage_of(u: Option<&Value>) -> Usage {
    let Some(u) = u else { return Usage::default() };
    let get = |k: &str| u.get(k).and_then(|x| x.as_u64());
    let prompt = match (get("input_tokens"), get("cache_creation_input_tokens"), get("cache_read_input_tokens")) {
        (None, None, None) => None,
        (a, b, c) => Some(a.unwrap_or(0) + b.unwrap_or(0) + c.unwrap_or(0)),
    };
    let completion = get("output_tokens");
    let total = match (prompt, completion) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
    };
    Usage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: total,
    }
}

fn first_tool_result_id(msg: &Value) -> Option<String> {
    msg.get("content")?
        .as_array()?
        .iter()
        .find(|b| b.get("type").and_then(|x| x.as_str()) == Some("tool_result"))
        .and_then(|b| b.get("tool_use_id"))
        .and_then(|x| x.as_str())
        .map(String::from)
}

fn user_messages(msg: &Value, out: &mut Vec<ChatMessage>) {
    match msg.get("content") {
        Some(Value::String(s)) => out.push(ChatMessage {
            role: "user".into(),
            content: Some(s.clone()),
            ..Default::default()
        }),
        Some(Value::Array(blocks)) => {
            let mut texts: Vec<String> = Vec::new();
            for b in blocks {
                match b.get("type").and_then(|x| x.as_str()) {
                    Some("tool_result") => out.push(ChatMessage {
                        role: "tool".into(),
                        content: Some(block_text(b.get("content"))),
                        tool_call_id: b
                            .get("tool_use_id")
                            .and_then(|x| x.as_str())
                            .map(String::from),
                        ..Default::default()
                    }),
                    Some("text") => {
                        if let Some(t) = b.get("text").and_then(|x| x.as_str()) {
                            texts.push(t.to_string());
                        }
                    }
                    Some("image") => texts.push("[image]".into()),
                    _ => {}
                }
            }
            if !texts.is_empty() {
                out.push(ChatMessage {
                    role: "user".into(),
                    content: Some(texts.join("\n")),
                    ..Default::default()
                });
            }
        }
        _ => {}
    }
}

fn block_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|b| match b.get("text").and_then(|x| x.as_str()) {
                Some(t) => t.to_string(),
                None => "[non-text content]".to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = concat!(
        r#"{"type":"summary","summary":"x","leafUuid":"z"}"#, "\n",
        r#"{"parentUuid":null,"sessionId":"s1","uuid":"u1","type":"user","timestamp":"2026-05-01T10:00:00.000Z","message":{"role":"user","content":"List files"}}"#, "\n",
        r#"{"parentUuid":"u1","sessionId":"s1","uuid":"a1","type":"assistant","timestamp":"2026-05-01T10:00:01.000Z","message":{"id":"m1","role":"assistant","model":"claude-sonnet-5-5","content":[{"type":"thinking","thinking":"hmm"}],"usage":{"input_tokens":3,"output_tokens":1,"cache_read_input_tokens":10}}}"#, "\n",
        r#"{"parentUuid":"a1","sessionId":"s1","uuid":"a2","type":"assistant","timestamp":"2026-05-01T10:00:02.000Z","message":{"id":"m1","role":"assistant","model":"claude-sonnet-5-5","content":[{"type":"tool_use","id":"toolu_1","name":"Task","input":{"prompt":"go"}}],"stop_reason":"tool_use","usage":{"input_tokens":3,"output_tokens":20,"cache_read_input_tokens":10}}}"#, "\n",
        r#"{"parentUuid":"a2","sessionId":"s1","uuid":"u2","type":"user","timestamp":"2026-05-01T10:00:09.000Z","toolUseResult":{"agentId":"ag1"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":[{"type":"text","text":"done"}]}]}}"#, "\n",
        r#"{"parentUuid":"u2","sessionId":"s1","uuid":"a3","type":"assistant","timestamp":"2026-05-01T10:00:10.000Z","message":{"id":"m2","role":"assistant","model":"claude-sonnet-5-5","content":[{"type":"text","text":"All done."}],"stop_reason":"end_turn"}}"#, "\n",
    );

    #[test]
    fn merges_streamed_blocks_into_one_call() {
        let out = parse(SESSION);
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        assert_eq!(out.events.len(), 2);
        let e = &out.events[0];
        assert_eq!(e.id, "a1");
        assert_eq!(e.request.messages[0].content.as_deref(), Some("List files"));
        let a = e.response.assistant.as_ref().unwrap();
        assert_eq!(a.reasoning.as_deref(), Some("hmm"));
        assert_eq!(a.tool_calls.as_ref().unwrap()[0].name, "Task");
        assert_eq!(e.response.usage.prompt_tokens, Some(13));
        assert_eq!(e.response.usage.total_tokens, Some(33));
        assert_eq!(e.response.finish_reason.as_deref(), Some("tool_use"));
        assert_eq!(e.duration_ms, 0);
    }

    #[test]
    fn tool_results_and_agent_links() {
        let out = parse(SESSION);
        let e = &out.events[1];
        assert_eq!(e.request.messages[0].role, "tool");
        assert_eq!(e.request.messages[0].tool_call_id.as_deref(), Some("toolu_1"));
        assert_eq!(e.request.messages[0].content.as_deref(), Some("done"));
        assert_eq!(e.raw["agent_links"]["ag1"], "toolu_1");
        assert_eq!(e.response.usage.total_tokens, None);
    }

    #[test]
    fn keeps_trailing_user_messages() {
        let src = format!(
            "{}{}\n",
            SESSION,
            r#"{"parentUuid":"a3","sessionId":"s1","uuid":"u3","type":"user","timestamp":"2026-05-01T10:01:00.000Z","message":{"role":"user","content":"thanks"}}"#
        );
        let out = parse(&src);
        assert_eq!(out.events.len(), 3);
        assert!(out.events[2].response.assistant.is_none());
    }
}
