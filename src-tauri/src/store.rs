use std::sync::RwLock;

use crate::model::Workspace;

#[derive(Default)]
pub struct AppStore {
    pub workspace: RwLock<Workspace>,
    pub watched_path: RwLock<Option<String>>,
}
