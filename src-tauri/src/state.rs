use app::account::Calendars;
use app::types::{AppCredentials, GoogleAuthToken, Preferences};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, PhysicalPosition, PhysicalSize};

pub struct TauriAppState {
    pub app: AppHandle,
}

#[derive(Default)]
pub struct AppState {
    pub google_auth_credentials: Mutex<GoogleAuthToken>,
    pub calendars: tokio::sync::Mutex<Calendars>,
    pub pending_events: Mutex<HashMap<String, google_calendar::types::Event>>,
    pub alert_size: Mutex<PhysicalSize<u32>>,
    pub alert_position: Mutex<PhysicalPosition<i32>>,
    pub app_config: Mutex<AppCredentials>,
    pub preferences: tokio::sync::Mutex<Preferences>,
}
