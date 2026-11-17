use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

use tauri::{State, Emitter, Manager};

use crate::export;
use crate::model::{LoadResult, SourceFile, Workspace};
use crate::parser;
use crate::session;
use crate::store::AppStore;

fn build_load_result(name: &str, path: &str, content: &str, known: &Workspace) -> LoadResult {
    let mut outcome = parser::parse(name, content);
    let mut files = vec![SourceFile {
        path: path.to_string(),
        name: name.to_string(),
        bytes: content.len() as u64,
        events: outcome.events.len() as u32,
    }];

    if path != name && outcome.events.iter().any(parser::claude_code::is_transcript_event) {
        for sub in parser::claude_code::companion_paths(Path::new(path)) {
            let sub_path = sub.to_string_lossy().into_owned();
            if known.files.iter().any(|f| f.path == sub_path) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&sub) else { continue };
            let sub_name = sub
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let sub_out = parser::claude_code::parse(&text);
            files.push(SourceFile {
                path: sub_path,
                name: sub_name,
                bytes: text.len() as u64,
                events: sub_out.events.len() as u32,
            });
            outcome.events.extend(sub_out.events);
            outcome.warnings.extend(sub_out.warnings);
        }
    }

    let sessions = session::group(&outcome.events);
    LoadResult {
        workspace: Workspace {
            files,
            events: outcome.events,
            sessions,
        },
        warnings: outcome.warnings,
    }
}

fn merge_workspace(ws: &mut Workspace, result: &LoadResult) {
    let mut seen: HashSet<String> = ws
        .events
        .iter()
        .filter(|e| parser::claude_code::is_transcript_event(e))
        .map(|e| e.id.clone())
        .collect();
    for f in &result.workspace.files {
        if f.path != f.name && ws.files.iter().any(|x| x.path == f.path) {
            continue;
        }
        ws.files.push(f.clone());
    }
    for e in &result.workspace.events {
        if parser::claude_code::is_transcript_event(e) && !seen.insert(e.id.clone()) {
            continue;
        }
        ws.events.push(e.clone());
    }
    ws.sessions = session::group(&ws.events);
}

fn load_into_state(
    state: &State<'_, AppStore>,
    name: &str,
    path: &str,
    content: &str,
    merge: bool,
) -> LoadResult {
    let mut ws = state.workspace.write().unwrap();
    if !merge {
        *ws = Workspace::default();
    }
    let mut result = build_load_result(name, path, content, &ws);
    merge_workspace(&mut ws, &result);
    result.workspace = ws.clone();
    result
}

#[tauri::command]
pub fn load_file(
    path: String,
    merge: Option<bool>,
    state: State<'_, AppStore>,
) -> Result<LoadResult, String> {
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let name = Path::new(&path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&path)
        .to_string();
    Ok(load_into_state(&state, &name, &path, &content, merge.unwrap_or(false)))
}

#[tauri::command]
pub fn load_text(
    name: String,
    content: String,
    merge: Option<bool>,
    state: State<'_, AppStore>,
) -> Result<LoadResult, String> {
    Ok(load_into_state(&state, &name, &name, &content, merge.unwrap_or(true)))
}

#[tauri::command]
pub fn export_session_markdown(
    session_id: String,
    state: State<'_, AppStore>,
) -> Result<String, String> {
    let ws = state.workspace.read().unwrap();
    export::session_to_markdown(&ws, &session_id).ok_or_else(|| "session not found".into())
}

#[tauri::command]
pub fn export_workspace_json(state: State<'_, AppStore>) -> Result<String, String> {
    let ws = state.workspace.read().unwrap();
    serde_json::to_string_pretty(&*ws).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_watch(
    path: String,
    state: State<'_, AppStore>,
    window: tauri::Window,
) -> Result<LoadResult, String> {
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let name = Path::new(&path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&path)
        .to_string();
    let result = build_load_result(&name, &path, &content, &Workspace::default());

    {
        let mut ws = state.workspace.write().unwrap();
        *ws = result.workspace.clone();
    }

    {
        let mut watched = state.watched_path.write().unwrap();
        *watched = Some(path.clone());
    }

    let app_handle = window.app_handle().clone();
    let path_clone = path.clone();
    thread::spawn(move || {
        let mut last_modified = None;
        let mut last_size = 0;

        if let Ok(metadata) = fs::metadata(&path_clone) {
            last_modified = metadata.modified().ok();
            last_size = metadata.len();
        }

        loop {
            {
                let app_store = app_handle.state::<AppStore>();
                let watched = app_store.watched_path.read().unwrap();
                if watched.as_ref() != Some(&path_clone) {
                    break;
                }
            }

            if let Ok(metadata) = fs::metadata(&path_clone) {
                let modified = metadata.modified().ok();
                let size = metadata.len();

                if modified != last_modified || size != last_size {
                    last_modified = modified;
                    last_size = size;

                    if let Ok(new_content) = fs::read_to_string(&path_clone) {
                        let f_name = Path::new(&path_clone)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or(&path_clone)
                            .to_string();
                        let load_res = build_load_result(&f_name, &path_clone, &new_content, &Workspace::default());

                        {
                            let app_store = app_handle.state::<AppStore>();
                            let mut ws = app_store.workspace.write().unwrap();
                            *ws = load_res.workspace.clone();
                        }

                        if let Err(e) = app_handle.emit("file-watch-update", load_res) {
                            eprintln!("Error emitting file-watch-update: {}", e);
                        }
                    }
                }
            }

            thread::sleep(Duration::from_millis(500));
        }
    });

    Ok(result)
}

#[tauri::command]
pub fn stop_watch(state: State<'_, AppStore>) -> Result<(), String> {
    let mut watched = state.watched_path.write().unwrap();
    *watched = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(uuid: &str, ty: &str, ts: &str, extra: &str, msg: &str) -> String {
        format!(
            r#"{{"parentUuid":null,"sessionId":"s","uuid":"{uuid}","type":"{ty}","timestamp":"{ts}"{extra},"message":{msg}}}"#
        )
    }

    fn main_file() -> String {
        [
            line("u1", "user", "2026-05-01T10:00:00Z", "", r#"{"role":"user","content":"go"}"#),
            line("a1", "assistant", "2026-05-01T10:00:01Z", "", r#"{"id":"m1","role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Task","input":{}}]}"#),
            line("u2", "user", "2026-05-01T10:30:00Z", r#","toolUseResult":{"agentId":"ag"}"#, r#"{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}"#),
        ]
        .join("\n")
    }

    fn sub_file() -> String {
        [
            line("su1", "user", "2026-05-01T10:00:02Z", r#","agentId":"ag""#, r#"{"role":"user","content":"sub task"}"#),
            line("sa1", "assistant", "2026-05-01T10:00:03Z", r#","agentId":"ag""#, r#"{"id":"sm1","role":"assistant","content":"sub done"}"#),
        ]
        .join("\n")
    }

    fn load(ws: &mut Workspace, name: &str, content: &str) {
        let r = build_load_result(name, name, content, ws);
        merge_workspace(ws, &r);
    }

    #[test]
    fn separate_loads_nest_subagent() {
        let mut ws = Workspace::default();
        load(&mut ws, "main.jsonl", &main_file());
        load(&mut ws, "agent-ag.jsonl", &sub_file());
        assert_eq!(ws.sessions.len(), 1);
        let conv = &ws.sessions[0].conversation;
        assert!(conv.iter().any(|t| t.agent_id.as_deref() == Some("ag")
            && t.parent_tool_call_id.as_deref() == Some("t1")));
    }

    #[test]
    fn reloading_does_not_duplicate_events() {
        let mut ws = Workspace::default();
        load(&mut ws, "main.jsonl", &main_file());
        load(&mut ws, "agent-ag.jsonl", &sub_file());
        let events = ws.events.len();
        let turns = ws.sessions[0].conversation.len();
        load(&mut ws, "agent-ag.jsonl", &sub_file());
        load(&mut ws, "main.jsonl", &main_file());
        assert_eq!(ws.events.len(), events);
        assert_eq!(ws.sessions[0].conversation.len(), turns);
    }

    #[test]
    fn companions_already_loaded_are_skipped() {
        let dir = std::env::temp_dir().join(format!("tlm-companion-{}", std::process::id()));
        let subdir = dir.join("s").join("subagents");
        fs::create_dir_all(&subdir).unwrap();
        let main_path = dir.join("s.jsonl");
        let sub_path = subdir.join("agent-ag.jsonl");
        fs::write(&main_path, main_file()).unwrap();
        fs::write(&sub_path, sub_file()).unwrap();

        let mp = main_path.to_string_lossy().into_owned();
        let sp = sub_path.to_string_lossy().into_owned();
        let mut ws = Workspace::default();
        let first = build_load_result("s.jsonl", &mp, &main_file(), &ws);
        assert_eq!(first.workspace.files.len(), 2);
        merge_workspace(&mut ws, &first);

        let again = build_load_result("s.jsonl", &mp, &main_file(), &ws);
        assert_eq!(again.workspace.files.len(), 1);
        merge_workspace(&mut ws, &again);
        let explicit = build_load_result("agent-ag.jsonl", &sp, &sub_file(), &ws);
        merge_workspace(&mut ws, &explicit);

        assert_eq!(ws.files.len(), 2);
        assert_eq!(ws.events.len(), 3);
        fs::remove_dir_all(&dir).ok();
    }
}
