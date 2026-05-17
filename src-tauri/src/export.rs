use std::fmt::Write;

use crate::model::Workspace;

pub fn session_to_markdown(ws: &Workspace, session_id: &str) -> Option<String> {
    let session = ws.sessions.iter().find(|s| s.id == session_id)?;
    let mut out = String::new();
    let _ = writeln!(out, "# Session {}", session.id);
    let _ = writeln!(out, "");
    let _ = writeln!(out, "- Client: `{}`", session.client);
    let _ = writeln!(out, "- Window: {} → {}", session.started_at, session.ended_at);
    let _ = writeln!(out, "- Events: {}", session.event_ids.len());
    let _ = writeln!(out, "");

    let models: std::collections::BTreeSet<_> = ws
        .events
        .iter()
        .filter(|e| session.event_ids.contains(&e.id))
        .map(|e| format!("{:?}/{}", e.provider, e.model))
        .collect();
    let _ = writeln!(out, "- Models: {}", models.into_iter().collect::<Vec<_>>().join(", "));
    let _ = writeln!(out, "");
    let _ = writeln!(out, "## Conversation");
    let _ = writeln!(out, "");

    for turn in &session.conversation {
        let _ = writeln!(out, "### {}", turn.role);
        if !turn.content.is_empty() {
            let _ = writeln!(out, "");
            let _ = writeln!(out, "{}", turn.content);
            let _ = writeln!(out, "");
        }
        if let Some(tcs) = &turn.tool_calls {
            for tc in tcs {
                let _ = writeln!(out, "**tool_call** `{}` — `{}`", tc.name, tc.arguments);
            }
            let _ = writeln!(out, "");
        }
    }

    Some(out)
}
