use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::fs;
use std::{collections::HashMap, path::PathBuf};

// todo: add Account Settings to support per account preferences (calenders to exclude, etc)
// todo: add field for app settings/preferences

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AppCredentials {
    // Todo: redact private data with secrecy package
    pub google_client_id: String,
    // Todo: redact private data with secrecy package
    pub google_calendar_api_key: String,
    pub google_redirect_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StateToken {
    pub token: GoogleAuthToken,
    pub disabled: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub verified_email: bool,
    pub name: String,
    pub given_name: String,
    pub family_name: String,
    pub picture: String,
    pub locale: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GoogleAuthToken {
    // Todo: redact private data with secrecy package
    pub access_token: String,
    pub token_type: String,
    #[serde(default, deserialize_with = "flexible_i64")]
    pub expires_in: i64,
    // Todo: redact private data with secrecy package
    pub refresh_token: Option<String>,
    pub scope: String,
    #[serde(default, deserialize_with = "flexible_opt_i64")]
    pub expires_at: Option<i64>,
    pub user: Option<UserInfo>, // extra_fields: EmptyExtraTokenFields,
    pub disabled: Option<bool>,
    /// Set when Google has revoked this account. A restart must not retry it.
    pub needs_signin: bool,
}

fn flexible_i64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    struct NumberOrString;
    impl<'de> Visitor<'de> for NumberOrString {
        type Value = i64;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an integer or a decimal string")
        }
        fn visit_i64<E: de::Error>(self, value: i64) -> Result<i64, E> {
            Ok(value)
        }
        fn visit_u64<E: de::Error>(self, value: u64) -> Result<i64, E> {
            Ok(value as i64)
        }
        fn visit_f64<E: de::Error>(self, value: f64) -> Result<i64, E> {
            Ok(value as i64)
        }
        fn visit_str<E: de::Error>(self, value: &str) -> Result<i64, E> {
            value.parse::<i64>().map_err(E::custom)
        }
    }
    deserializer.deserialize_any(NumberOrString)
}

fn flexible_opt_i64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<i64>, D::Error> {
    struct NumberOrString;
    impl<'de> Visitor<'de> for NumberOrString {
        type Value = Option<i64>;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an integer, a decimal string, or null")
        }
        fn visit_unit<E: de::Error>(self) -> Result<Option<i64>, E> {
            Ok(None)
        }
        fn visit_none<E: de::Error>(self) -> Result<Option<i64>, E> {
            Ok(None)
        }
        fn visit_i64<E: de::Error>(self, value: i64) -> Result<Option<i64>, E> {
            Ok(Some(value))
        }
        fn visit_u64<E: de::Error>(self, value: u64) -> Result<Option<i64>, E> {
            Ok(Some(value as i64))
        }
        fn visit_str<E: de::Error>(self, value: &str) -> Result<Option<i64>, E> {
            if value.is_empty() {
                return Ok(None);
            }
            value.parse::<i64>().map(Some).map_err(E::custom)
        }
    }
    deserializer.deserialize_any(NumberOrString)
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Preferences {
    storage_path: String,
    notify_only_meetings: bool,
    accounts_preferences: HashMap<String, AccountPreference>,
    /// Settings written by another version. Kept through later saves.
    #[serde(flatten)]
    extra: HashMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AccountPreference {
    pub hidden_calendars: Vec<String>,
}

impl Preferences {
    pub async fn load_from_file(app_path: PathBuf) -> Result<Self, String> {
        let storage_path: PathBuf = app_path.join("preferences.json");
        if !storage_path.is_file() {
            let mut preferences = Preferences::default();
            preferences.storage_path = path_string(&storage_path);
            preferences.save_state().await?;
            return Ok(preferences);
        }
        match fs::read(&storage_path) {
            Ok(bytes) => match decode_preferences(&bytes, &storage_path) {
                Ok(preferences) => Ok(preferences),
                Err(()) => {
                    let _ = crate::storage::quarantine_bytes(&storage_path, &bytes);
                    restore_preferences_backup(&storage_path).await
                }
            },
            Err(err) => {
                println!("Error reading preferences {:?}", &err);
                let _ = crate::storage::quarantine_path(&storage_path);
                restore_preferences_backup(&storage_path).await
            }
        }
    }

    pub async fn save_state(&self) -> Result<(), String> {
        if self.storage_path.is_empty() {
            return Err("preferences path is empty".to_string());
        }
        let path: PathBuf = self.storage_path.clone().into();
        let bytes = serde_json::to_vec(self).map_err(|err| err.to_string())?;
        println!("Preferences::Save state {:?}", self);
        crate::storage::atomic_write(&path, &bytes).map_err(|err| err.to_string())
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
    pub fn notify_only_meetings(&self) -> bool {
        self.notify_only_meetings
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
            extra: self.extra.clone(),
        }
    }
}

fn path_string(path: &PathBuf) -> String {
    path.to_str().unwrap_or("").to_string()
}

/// Read known fields one at a time. A bad value falls back to the default for
/// that field, and keys this version does not understand stay in `extra`.
fn decode_preferences(bytes: &[u8], path: &PathBuf) -> Result<Preferences, ()> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ())?;
    let object = value.as_object().ok_or(())?;
    let mut preferences = Preferences::default();
    preferences.storage_path = path_string(path);
    if let Some(flag) = object.get("notify_only_meetings").and_then(Value::as_bool) {
        preferences.notify_only_meetings = flag;
    }
    if let Some(accounts) = object
        .get("accounts_preferences")
        .and_then(Value::as_object)
    {
        for (email, preference) in accounts {
            let hidden = preference
                .get("hidden_calendars")
                .and_then(Value::as_array)
                .map(|ids| {
                    ids.iter()
                        .filter_map(|id| id.as_str().map(|id| id.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            preferences.accounts_preferences.insert(
                email.clone(),
                AccountPreference {
                    hidden_calendars: hidden,
                },
            );
        }
    }
    for (key, value) in object {
        if matches!(
            key.as_str(),
            "storage_path" | "notify_only_meetings" | "accounts_preferences"
        ) {
            continue;
        }
        preferences.extra.insert(key.clone(), value.clone());
    }
    Ok(preferences)
}

async fn restore_preferences_backup(path: &PathBuf) -> Result<Preferences, String> {
    let backup = crate::storage::backup_path(path);
    if let Ok(bytes) = fs::read(&backup) {
        if decode_preferences(&bytes, path).is_ok() {
            // The damaged file is still in place and is not valid JSON, so this
            // write quarantines it again and does not replace the good backup.
            crate::storage::atomic_write(path, &bytes).map_err(|err| err.to_string())?;
            return decode_preferences(&bytes, path).map_err(|_| "backup preferences".to_string());
        }
    }
    let mut preferences = Preferences::default();
    preferences.storage_path = path_string(path);
    preferences.save_state().await?;
    Ok(preferences)
}
