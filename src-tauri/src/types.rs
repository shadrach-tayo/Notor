use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, PhysicalPosition, PhysicalSize};
use crate::account::CalenderAccount;


pub struct TauriAppState {
    pub app: AppHandle,
}
// todo: restructure app state to support multiple accounts(auth credentials)
// todo: add Account Settings to support per account preferences (calenders to exclude, etc)
// todo: add field for app settings/preferences


#[derive(Default)]
pub struct AppState {
    pub google_auth_credentials: Mutex<GoogleAuthToken>,
    pub accounts: Mutex<Vec<CalenderAccount>>,
    pub pending_events: Mutex<HashMap<String, google_calendar::types::Event>>,
    pub alert_size: Mutex<PhysicalSize<u32>>,
    pub alert_position: Mutex<PhysicalPosition<i32>>,
    pub app_config: Mutex<AppCredentials>,
    pub api_url: Mutex<String>,
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
    pub expires_in: i64,
    pub refresh_token: Option<String>,
    pub scope: String,
    pub expires_at: Option<i64>, // extra_fields: EmptyExtraTokenFields,
}
