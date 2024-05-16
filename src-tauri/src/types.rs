use crate::account::Calendars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, PhysicalPosition, PhysicalSize};

pub struct TauriAppState {
    pub app: AppHandle,
}

// todo: add Account Settings to support per account preferences (calenders to exclude, etc)
// todo: add field for app settings/preferences

#[derive(Default)]
pub struct AppState {
    pub google_auth_credentials: Mutex<GoogleAuthToken>,
    // pub accounts: Mutex<Vec<CalenderAccount>>,
    pub calendars: tokio::sync::Mutex<Calendars>,
    pub pending_events: Mutex<HashMap<String, google_calendar::types::Event>>,
    pub alert_size: Mutex<PhysicalSize<u32>>,
    pub alert_position: Mutex<PhysicalPosition<i32>>,
    pub app_config: Mutex<AppCredentials>,
    pub api_url: Mutex<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AppCredentials {
    // Todo: redact private data with secrecy package
    pub google_client_id: String,
    // Todo: redact private data with secrecy package
    pub google_client_secret: String,
    // Todo: redact private data with secrecy package
    pub google_calendar_api_key: String,
    pub google_redirect_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct StateToken {
    pub token: GoogleAuthToken,
    pub disabled: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub verified_email: bool,
    pub name: String,
    pub given_name: String,
    pub family_name: String,
    pub picture: String,
    pub locale: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct GoogleAuthToken {
    // Todo: redact private data with secrecy package
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    // Todo: redact private data with secrecy package
    pub refresh_token: Option<String>,
    pub scope: String,
    pub expires_at: Option<i64>,
    pub user: Option<UserInfo>, // extra_fields: EmptyExtraTokenFields,
    pub disabled: Option<bool>,
}
