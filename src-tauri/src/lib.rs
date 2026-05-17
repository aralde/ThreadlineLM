mod commands;
mod export;
mod model;
mod parser;
mod session;
mod store;

use store::AppStore;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppStore::default())
        .invoke_handler(tauri::generate_handler![
            commands::load_file,
            commands::load_text,
            commands::export_session_markdown,
            commands::export_workspace_json,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ThreadlineLM");
}
