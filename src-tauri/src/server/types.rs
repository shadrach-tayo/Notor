use std::sync::Mutex;
use serde::{Serialize, Deserialize};
use tauri::AppHandle;

pub struct TauriAppState {
    pub app: Mutex<AppHandle>,
}

#[derive(Default)]
pub struct AppState {
  pub google_auth_credentials: std::sync::Mutex<GoogleAuthToken>,
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