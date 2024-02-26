use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, PhysicalPosition, PhysicalSize};

pub struct TauriAppState {
    pub app: Mutex<AppHandle>,
}

#[derive(Default)]
pub struct AppState {
    pub google_auth_credentials: Mutex<GoogleAuthToken>,
    pub alert_size: Mutex<PhysicalSize<u32>>,
    pub alert_position: Mutex<PhysicalPosition<i32>>,
    pub app_config: Mutex<AppCredentials>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AppCredentials {
    pub google_client_id: String,
    pub google_client_secret: String,
    pub google_calendar_api_key: String,
    pub google_redirect_url: String,
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
