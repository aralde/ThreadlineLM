use chrono::DateTime;
use sha2::{Digest, Sha256};

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
        let ip = client_ip(&e.client);
        let ts_ms = parse_ms(&e.ts).unwrap_or(0);

        let reuse = current_by_ip.get(&ip).and_then(|&si| {
            let s = &sessions[si];
            let last_ts = parse_ms(&s.ended_at).unwrap_or(0);
            if ts_ms - last_ts <= INACTIVITY_WINDOW_MS {
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
                    client: ip.clone(),
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
        s.conversation = reconstruct(events, &s.event_ids);
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
                    });
                }
            }
        }
    }

    out
}
