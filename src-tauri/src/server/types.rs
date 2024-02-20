use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, PhysicalPosition, PhysicalSize};

pub struct TauriAppState {
    pub app: Mutex<AppHandle>,
}

#[derive(Default)]
pub struct AppState {
    pub google_auth_credentials: std::sync::Mutex<GoogleAuthToken>,
    pub alert_size: std::sync::Mutex<PhysicalSize<u32>>,
    pub alert_position: std::sync::Mutex<PhysicalPosition<i32>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct GoogleAuthToken {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub refresh_token: Option<String>,
    pub scope: String,
    pub expires_at: Option<u64>, // extra_fields: EmptyExtraTokenFields,
}
