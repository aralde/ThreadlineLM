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
    let result = build_load_result(&name, &path, &content);

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
                        let load_res = build_load_result(&f_name, &path_clone, &new_content);

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
