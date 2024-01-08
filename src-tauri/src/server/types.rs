use std::sync::Mutex;
use tauri::AppHandle;

pub struct TauriAppState {
    pub app: Mutex<AppHandle>,
}
