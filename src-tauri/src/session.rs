use chrono::DateTime;
use sha2::{Digest, Sha256};

use crate::parser::claude_code;
use crate::model::{ChatMessage, ConversationTurn, LogEvent, Session};

const INACTIVITY_WINDOW_MS: i64 = 60 * 1000;

pub fn group(events: &[LogEvent]) -> Vec<Session> {
    let mut idx: Vec<usize> = (0..events.len()).collect();
    idx.sort_by(|&a, &b| events[a].ts.cmp(&events[b].ts));

    // Group by client-ip (strip port) within inactivity window.
    let mut sessions: Vec<Session> = Vec::new();
    let mut current_by_ip: std::collections::HashMap<String, usize> = Default::default();

    for &i in &idx {
        let e = &events[i];
        let transcript = claude_code::is_transcript_event(e);
        let ip = if transcript {
            format!("claude-code:{}", e.client)
        } else {
            client_ip(&e.client)
        };
        let ts_ms = parse_ms(&e.ts).unwrap_or(0);

        let reuse = current_by_ip.get(&ip).and_then(|&si| {
            let s = &sessions[si];
            let last_ts = parse_ms(&s.ended_at).unwrap_or(0);
            if transcript || ts_ms - last_ts <= INACTIVITY_WINDOW_MS {
                Some(si)
            } else {
                None
            }
        });

        let si = match reuse {
            Some(si) => si,
            None => {
                let id = hash_id(&format!("{}|{}", ip, e.ts));
                sessions.push(Session {
                    id,
                    client: ip.strip_prefix("claude-code:").unwrap_or(&ip).to_string(),
                    started_at: e.ts.clone(),
                    ended_at: e.ts.clone(),
                    event_ids: Vec::new(),
                    conversation: Vec::new(),
                });
                let si = sessions.len() - 1;
                current_by_ip.insert(ip.clone(), si);
                si
            }
        };

        sessions[si].event_ids.push(e.id.clone());
        sessions[si].ended_at = e.ts.clone();
    }

    for s in &mut sessions {
        let by_id: std::collections::HashMap<&str, &LogEvent> =
            events.iter().map(|e| (e.id.as_str(), e)).collect();
        let is_transcript = s
            .event_ids
            .first()
            .and_then(|id| by_id.get(id.as_str()))
            .map_or(false, |e| claude_code::is_transcript_event(e));
        s.conversation = if is_transcript {
            reconstruct_transcript(&by_id, &s.event_ids)
        } else {
            reconstruct(events, &s.event_ids)
        };
    }
    sessions
}

fn client_ip(client: &str) -> String {
    client.split(':').next().unwrap_or(client).to_string()
}

fn parse_ms(ts: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|d| d.timestamp_millis())
}

fn hash_id(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(&h.finalize()[..8])
}

fn reconstruct(events: &[LogEvent], event_ids: &[String]) -> Vec<ConversationTurn> {
    let by_id: std::collections::HashMap<&str, &LogEvent> =
        events.iter().map(|e| (e.id.as_str(), e)).collect();

    // The "winner" request is the one with the most messages: in agentic loops
    // the client accumulates history (user, assistant tool_calls, tool results,
    // etc.) so the last request already contains every earlier turn.
    let mut winner_event_id: Option<&str> = None;
    let mut winner_msgs: Option<&Vec<ChatMessage>> = None;
    for id in event_ids {
        if let Some(e) = by_id.get(id.as_str()) {
            let ms = &e.request.messages;
            if winner_msgs.map(|m| ms.len() > m.len()).unwrap_or(true) {
                winner_msgs = Some(ms);
                winner_event_id = Some(e.id.as_str());
            }
        }
    }

    let mut out: Vec<ConversationTurn> = Vec::new();
    if let (Some(msgs), Some(eid)) = (winner_msgs, winner_event_id) {
        for m in msgs {
            out.push(ConversationTurn {
                event_id: eid.to_string(),
                role: m.role.clone(),
                content: m.content.clone().unwrap_or_default(),
                tool_calls: m.tool_calls.clone(),
                reasoning: m.reasoning.clone(),
                ..Default::default()
            });
        }
        // Append the final assistant from the winning event (not present in its
        // own request messages because it is the response).
        if let Some(e) = by_id.get(eid) {
            if let Some(a) = &e.response.assistant {
                out.push(ConversationTurn {
                    event_id: e.id.clone(),
                    role: a.role.clone(),
                    content: a.content.clone().unwrap_or_default(),
                    tool_calls: a.tool_calls.clone(),
                    reasoning: a.reasoning.clone(),
                    ..Default::default()
                });
            }
        }
    } else {
        // Fallback: no requests had messages, just stitch every response.
        for id in event_ids {
            if let Some(e) = by_id.get(id.as_str()) {
                if let Some(a) = &e.response.assistant {
                    out.push(ConversationTurn {
                        event_id: e.id.clone(),
                        role: a.role.clone(),
                        content: a.content.clone().unwrap_or_default(),
                        tool_calls: a.tool_calls.clone(),
                        reasoning: a.reasoning.clone(),
                        ..Default::default()
                    });
                }
            }
        }
    }

    out
}

fn event_agent(e: &LogEvent) -> Option<&str> {
    e.raw.get("agent_id").and_then(|v| v.as_str())
}

fn push_event_turns(
    e: &LogEvent,
    agent: Option<&str>,
    parent: Option<&str>,
    out: &mut Vec<ConversationTurn>,
) {
    let mk = |m: &ChatMessage, focus: String| ConversationTurn {
        event_id: e.id.clone(),
        role: m.role.clone(),
        content: m.content.clone().unwrap_or_default(),
        tool_calls: m.tool_calls.clone(),
        reasoning: m.reasoning.clone(),
        focus: Some(focus),
        agent_id: agent.map(String::from),
        parent_tool_call_id: parent.map(String::from),
    };
    for (i, m) in e.request.messages.iter().enumerate() {
        out.push(mk(m, format!("msg:{i}")));
    }
    if let Some(a) = &e.response.assistant {
        out.push(mk(a, "resp".into()));
    }
}

/// Transcript events carry only the messages added since the previous call, so
/// the conversation is the concatenation of all of them. Subagent events are
/// nested right after the tool call that spawned them.
fn reconstruct_transcript(
    by_id: &std::collections::HashMap<&str, &LogEvent>,
    event_ids: &[String],
) -> Vec<ConversationTurn> {
    let events: Vec<&LogEvent> = event_ids
        .iter()
        .filter_map(|id| by_id.get(id.as_str()).copied())
        .collect();

    let mut links: std::collections::HashMap<String, String> = Default::default();
    for e in &events {
        if let Some(m) = e.raw.get("agent_links").and_then(|v| v.as_object()) {
            for (agent, tool) in m {
                if let Some(t) = tool.as_str() {
                    links.entry(agent.clone()).or_insert_with(|| t.to_string());
                }
            }
        }
    }

    let mut sub: std::collections::BTreeMap<String, Vec<&LogEvent>> = Default::default();
    for e in &events {
        if let Some(a) = event_agent(e) {
            sub.entry(a.to_string()).or_default().push(e);
        }
    }

    let mut by_tool: std::collections::HashMap<String, Vec<String>> = Default::default();
    for agent in sub.keys() {
        match links.get(agent) {
            Some(t) => by_tool.entry(t.clone()).or_default().push(agent.clone()),
            None => {}
        }
    }

    let emit_agent = |agent: &str, parent: Option<&str>, out: &mut Vec<ConversationTurn>| {
        for e in &sub[agent] {
            push_event_turns(e, Some(agent), parent, out);
        }
    };

    let mut emitted: std::collections::HashSet<String> = Default::default();
    let mut out = Vec::new();
    for e in events.iter().filter(|e| event_agent(e).is_none()) {
        push_event_turns(e, None, None, &mut out);
        if let Some(calls) = e.response.assistant.as_ref().and_then(|a| a.tool_calls.as_ref()) {
            for c in calls {
                for agent in by_tool.get(&c.id).into_iter().flatten() {
                    if emitted.insert(agent.clone()) {
                        emit_agent(agent, Some(&c.id), &mut out);
                    }
                }
            }
        }
    }
    for agent in sub.keys() {
        if !emitted.contains(agent) {
            emit_agent(agent, None, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::claude_code;

    fn line(session: &str, uuid: &str, ty: &str, ts: &str, extra: &str, msg: &str) -> String {
        format!(
            r#"{{"parentUuid":null,"sessionId":"{session}","uuid":"{uuid}","type":"{ty}","timestamp":"{ts}"{extra},"message":{msg}}}"#
        )
    }

    #[test]
    fn nests_subagent_under_spawning_tool_call() {
        let main = [
            line("s", "u1", "user", "2026-05-01T10:00:00Z", "", r#"{"role":"user","content":"go"}"#),
            line("s", "a1", "assistant", "2026-05-01T10:00:01Z", "", r#"{"id":"m1","role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Task","input":{}}]}"#),
            line("s", "u2", "user", "2026-05-01T10:30:00Z", r#","toolUseResult":{"agentId":"ag"}"#, r#"{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}"#),
            line("s", "a2", "assistant", "2026-05-01T10:30:01Z", "", r#"{"id":"m2","role":"assistant","content":"fin"}"#),
        ]
        .join("\n");
        let subf = [
            line("s", "su1", "user", "2026-05-01T10:00:02Z", r#","agentId":"ag""#, r#"{"role":"user","content":"sub task"}"#),
            line("s", "sa1", "assistant", "2026-05-01T10:00:03Z", r#","agentId":"ag""#, r#"{"id":"sm1","role":"assistant","content":"sub done"}"#),
        ]
        .join("\n");
        let mut events = claude_code::parse(&main).events;
        events.extend(claude_code::parse(&subf).events);
        let sessions = group(&events);
        assert_eq!(sessions.len(), 1);
        let conv = &sessions[0].conversation;
        let roles: Vec<(&str, Option<&str>)> = conv
            .iter()
            .map(|t| (t.role.as_str(), t.agent_id.as_deref()))
            .collect();
        assert_eq!(
            roles,
            vec![
                ("user", None),
                ("assistant", None),
                ("user", Some("ag")),
                ("assistant", Some("ag")),
                ("tool", None),
                ("assistant", None),
            ]
        );
        assert_eq!(conv[2].parent_tool_call_id.as_deref(), Some("t1"));
        assert_eq!(conv[5].focus.as_deref(), Some("resp"));
    }
}
