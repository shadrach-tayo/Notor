use crate::account::Calendars;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::sync::Mutex;
use std::{collections::HashMap, path::PathBuf};
use tauri::{AppHandle, PhysicalPosition, PhysicalSize};

pub struct TauriAppState {
    pub app: AppHandle,
}

// todo: add Account Settings to support per account preferences (calenders to exclude, etc)
// todo: add field for app settings/preferences

#[derive(Default)]
pub struct AppState {
    pub google_auth_credentials: Mutex<GoogleAuthToken>,
    pub calendars: tokio::sync::Mutex<Calendars>,
    pub pending_events: Mutex<HashMap<String, google_calendar::types::Event>>,
    pub alert_size: Mutex<PhysicalSize<u32>>,
    pub alert_position: Mutex<PhysicalPosition<i32>>,
    pub app_config: Mutex<AppCredentials>,
    pub api_url: Mutex<String>,
    pub preferences: tokio::sync::Mutex<Preferences>,
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

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Preferences {
    storage_path: String,
    notify_only_meetings: bool,
    accounts_preferences: HashMap<String, AccountPreference>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AccountPreference {
    pub hidden_calendars: Vec<String>,
}

impl Preferences {
    pub async fn load_from_file(app_path: PathBuf) -> Result<Self, String> {
        let storage_path: PathBuf = app_path.join("preferences.json");
        if storage_path.is_file() {
            let preferences = match fs::read_to_string(storage_path.clone()) {
                Ok(settings) => {
                    serde_json::from_str::<Preferences>(&settings).map_err(|err| err.to_string())
                }
                Err(err) => {
                    println!("Error {:?}", &err);
                    Err(err.to_string())
                }
            };

            let preferences = preferences.unwrap_or(Preferences::default());
            println!("Preferences {:?}", &preferences);
            Ok(Preferences {
                storage_path: storage_path
                    .to_str()
                    .map_or(String::from(""), |value| value.to_string()),
                ..preferences
            })
        } else {
            let mut file = fs::File::create(storage_path).map_err(|err| err.to_string())?;
            let mut bytes: Vec<u8> = Vec::new();
            serde_json::to_writer(&mut bytes, &Preferences::default()).unwrap();
            let _ = match file.write(&bytes) {
                Ok(_size) => Ok(()),
                Err(err) => {
                    println!("Error seeding default preferences");
                    Err(err.to_string())
                }
            };
            Ok(Self::default())
        }
    }

    pub async fn save_state(&self) -> Result<(), String> {
        let path: PathBuf = self.storage_path.clone().into();

        let mut file = fs::File::create(path).map_err(|err| err.to_string())?;
        let mut bytes: Vec<u8> = Vec::new();
        serde_json::to_writer(&mut bytes, self).unwrap();
        println!("Preferences::Save state {:?}", self);
        let _ = match file.write(&bytes) {
            Ok(_size) => Ok(()),
            Err(err) => {
                println!("Error seeding default preferences");
                Err(err.to_string())
            }
        };

        Ok(())
    }

    pub fn get_account_preference(&self, account_email: &str) -> AccountPreference {
        self.accounts_preferences
            .get(account_email)
            .map_or(AccountPreference::default(), |preferences| {
                preferences.to_owned()
            })
        // self.accounts_preferences
        //     .lock()
        //     .unwrap()
        //     .get(account_email)
        //     .map_or(AccountPreference::default(), |preferences| {
        //         preferences.to_owned()
        //     })
    }

    pub fn is_calendar_hidden(&self, account: &str, calendar_id: &String) -> bool {
        if let Some(account_pref) = self.accounts_preferences.get(account) {
            account_pref.hidden_calendars.contains(calendar_id)
        } else {
            false
        }
    }
    pub async fn set_notify_only_meetings(&mut self, value: bool) {
        self.notify_only_meetings = value;
        let _ = self.save_state().await;
    }

    pub async fn hide_calendar(
        &mut self,
        account: String,
        calendar_id: String,
    ) -> Result<(), String> {
        if self.accounts_preferences.get(&account).is_none() {
            self.accounts_preferences
                .insert(account.clone(), AccountPreference::default());
        }

        let account_pref = self.accounts_preferences.get_mut(&account).unwrap();

        if !account_pref.hidden_calendars.contains(&calendar_id) {
            account_pref.hidden_calendars.push(calendar_id.into());
        }

        let _ = self.save_state().await;
        Ok(())
    }

    pub async fn show_calendar(
        &mut self,
        account: String,
        calendar_id: String,
    ) -> Result<(), String> {
        if self.accounts_preferences.get(&account).is_none() {
            self.accounts_preferences
                .insert(account.clone(), AccountPreference::default());
        }

        let account_pref = self.accounts_preferences.get_mut(&account).unwrap();

        let list = account_pref.hidden_calendars.clone();
        account_pref.hidden_calendars = list
            .iter()
            .filter_map(|cal| {
                if cal.as_str() != &calendar_id {
                    Some(cal.to_string())
                } else {
                    None
                }
            })
            // .filter(|calendar| calendar.as_str() != &calendar_id)
            // .map(|cal| cal.to_string())
            .collect::<Vec<String>>();

        let _ = self.save_state().await;
        Ok(())
    }

    pub fn get_state(&self) -> Preferences {
        Preferences {
            storage_path: String::from("[redacted]"),
            notify_only_meetings: self.notify_only_meetings.clone(),
            accounts_preferences: self.accounts_preferences.clone(),
        }
    }
}
