use std::fs;
use std::path::Path;

use tauri::State;

use crate::export;
use crate::model::{LoadResult, SourceFile, Workspace};
use crate::parser;
use crate::session;
use crate::store::AppStore;

fn build_load_result(name: &str, path: &str, content: &str) -> LoadResult {
    let outcome = parser::parse(name, content);
    let sessions = session::group(&outcome.events);
    let files = vec![SourceFile {
        path: path.to_string(),
        name: name.to_string(),
        bytes: content.len() as u64,
        events: outcome.events.len() as u32,
    }];
    LoadResult {
        workspace: Workspace {
            files,
            events: outcome.events,
            sessions,
        },
        warnings: outcome.warnings,
    }
}

#[tauri::command]
pub fn load_file(path: String, state: State<'_, AppStore>) -> Result<LoadResult, String> {
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let name = Path::new(&path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&path)
        .to_string();
    let result = build_load_result(&name, &path, &content);
    merge_into_state(&state, &result);
    Ok(result)
}

#[tauri::command]
pub fn load_text(
    name: String,
    content: String,
    state: State<'_, AppStore>,
) -> Result<LoadResult, String> {
    let result = build_load_result(&name, &name, &content);
    merge_into_state(&state, &result);
    Ok(result)
}

fn merge_into_state(state: &State<'_, AppStore>, result: &LoadResult) {
    let mut ws = state.workspace.write().unwrap();
    ws.files.extend(result.workspace.files.clone());
    ws.events.extend(result.workspace.events.clone());
    // Recompute sessions over the merged event set.
    ws.sessions = session::group(&ws.events);
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
