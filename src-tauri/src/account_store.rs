//! Load and save `notor_accounts.json` without dropping accounts on a bad read.
//!
//! A clean file, including an empty one, is trusted and left untouched. A file
//! cut short keeps every complete entry and fills the gap from `.bak` or the
//! legacy single-account files. Entries that cannot be used yet are written
//! back unchanged.

use crate::storage::{self, atomic_write, complete_objects};
use crate::types::GoogleAuthToken;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct StoredAccount {
    pub raw: Value,
    pub token: GoogleAuthToken,
    /// False when the entry could not be parsed. It is written back as `raw`.
    pub parsed: bool,
    pub dirty: bool,
}

#[derive(Clone, Debug)]
pub struct AccountStore {
    pub entries: Vec<StoredAccount>,
    ready: bool,
    dirty: bool,
    /// Bytes of a clean file. A clean store is not written back, so this stays
    /// on disk unchanged and is not read again.
    #[allow(dead_code)]
    original: Option<Vec<u8>>,
}

impl AccountStore {
    /// A store that refuses saves. Startup uses this until loading finishes.
    pub fn loading() -> Self {
        Self {
            entries: Vec::new(),
            ready: false,
            dirty: false,
            original: None,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn load(dir: &Path) -> Self {
        let path = accounts_path(dir);
        let backup = storage::backup_path(&path);
        let legacy = read_legacy(dir);
        match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(Value::Array(items)) => Self {
                    entries: items.iter().map(parse_entry).collect(),
                    ready: true,
                    dirty: false,
                    original: Some(bytes),
                },
                _ => {
                    let _ = storage::quarantine_bytes(&path, &bytes);
                    let mut entries: Vec<StoredAccount> =
                        complete_objects(&bytes).iter().map(parse_entry).collect();
                    fill_missing(&mut entries, &backup, &legacy);
                    Self {
                        entries,
                        ready: true,
                        dirty: true,
                        original: None,
                    }
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let mut entries = entries_from_backup(&backup);
                if entries.is_empty() {
                    entries = legacy;
                } else {
                    fill_missing(&mut entries, &backup, &legacy);
                }
                Self {
                    dirty: !entries.is_empty(),
                    entries,
                    ready: true,
                    original: None,
                }
            }
            Err(_) => {
                let dest = storage::quarantine_path(&path);
                let _ = fs::copy(&path, dest);
                let mut entries = entries_from_backup(&backup);
                fill_missing(&mut entries, &backup, &legacy);
                if entries.is_empty() {
                    entries = legacy;
                }
                Self {
                    entries,
                    ready: true,
                    dirty: true,
                    original: None,
                }
            }
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        if !self.ready {
            return Err("startup loading has not finished".to_string());
        }
        if !self.dirty && self.entries.iter().all(|entry| !entry.dirty) {
            return Ok(());
        }
        let values: Vec<Value> = self
            .entries
            .iter()
            .map(|entry| {
                if entry.dirty && entry.parsed {
                    merge_entry(&entry.raw, &entry.token)
                } else {
                    entry.raw.clone()
                }
            })
            .collect();
        let bytes = serde_json::to_vec(&values).map_err(|err| err.to_string())?;
        atomic_write(&accounts_path(dir), &bytes).map_err(|err| err.to_string())
    }

    pub fn mark_ready(&mut self) {
        self.ready = true;
    }

    pub fn tokens(&self) -> Vec<GoogleAuthToken> {
        self.entries
            .iter()
            .filter(|entry| entry.parsed)
            .map(|entry| entry.token.clone())
            .collect()
    }

    pub fn update_token(&mut self, email: &str, token: GoogleAuthToken) -> bool {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry_email(entry) == email)
        else {
            return false;
        };
        entry.token = token;
        entry.parsed = true;
        entry.dirty = true;
        entry.raw = merge_entry(&entry.raw, &entry.token);
        true
    }

    pub fn upsert(&mut self, token: GoogleAuthToken) {
        let email = token
            .user
            .as_ref()
            .map(|user| user.email.clone())
            .unwrap_or_default();
        if !email.is_empty() {
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry_email(entry) == email)
            {
                entry.token = token;
                entry.parsed = true;
                entry.dirty = true;
                entry.raw = merge_entry(&entry.raw, &entry.token);
                return;
            }
        }
        let raw = serde_json::json!({ "token": token });
        self.entries.push(StoredAccount {
            token,
            raw,
            parsed: true,
            dirty: true,
        });
        self.dirty = true;
    }

    pub fn remove(&mut self, email: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry_email(entry) != email);
        let removed = self.entries.len() != before;
        if removed {
            self.dirty = true;
        }
        removed
    }
}

pub fn accounts_path(dir: &Path) -> PathBuf {
    dir.join("notor_accounts.json")
}

fn entry_email(entry: &StoredAccount) -> String {
    entry
        .token
        .user
        .as_ref()
        .map(|user| user.email.clone())
        .filter(|email| !email.is_empty())
        .or_else(|| email_from_raw(&entry.raw))
        .unwrap_or_default()
}

fn email_from_raw(raw: &Value) -> Option<String> {
    let token = raw.get("token").unwrap_or(raw);
    token
        .pointer("/user/email")
        .and_then(Value::as_str)
        .filter(|email| !email.is_empty())
        .map(|email| email.to_string())
}

fn parse_entry(value: &Value) -> StoredAccount {
    let token_value = value.get("token").cloned().unwrap_or_else(|| value.clone());
    match serde_json::from_value::<GoogleAuthToken>(token_value) {
        Ok(mut token) => {
            if token.disabled.is_none() {
                if let Some(flag) = value.get("disabled").and_then(Value::as_bool) {
                    token.disabled = Some(flag);
                }
            }
            StoredAccount {
                raw: value.clone(),
                token,
                parsed: true,
                dirty: false,
            }
        }
        Err(_) => StoredAccount {
            raw: value.clone(),
            token: GoogleAuthToken::default(),
            parsed: false,
            dirty: false,
        },
    }
}

fn merge_entry(raw: &Value, token: &GoogleAuthToken) -> Value {
    let token_value = serde_json::to_value(token).unwrap_or(Value::Null);
    if raw.get("token").is_some() || raw.get("access_token").is_none() {
        let mut wrapper = if raw.is_object() {
            raw.clone()
        } else {
            serde_json::json!({})
        };
        let existing = wrapper.get("token").cloned().unwrap_or(Value::Null);
        wrapper["token"] = merge_objects(existing, token_value);
        wrapper
    } else {
        serde_json::json!({ "token": merge_objects(raw.clone(), token_value) })
    }
}

fn merge_objects(existing: Value, update: Value) -> Value {
    match (existing, update) {
        (Value::Object(mut dst), Value::Object(src)) => {
            for (key, value) in src {
                if key == "user" {
                    if let Some(current) = dst.get(&key).cloned() {
                        dst.insert(key, merge_objects(current, value));
                        continue;
                    }
                }
                dst.insert(key, value);
            }
            Value::Object(dst)
        }
        (_existing, update) => update,
    }
}

fn fill_missing(entries: &mut Vec<StoredAccount>, backup: &Path, legacy: &[StoredAccount]) {
    let mut seen = emails(entries);
    if let Ok(bytes) = fs::read(backup) {
        if let Ok(Value::Array(items)) = serde_json::from_slice::<Value>(&bytes) {
            for item in items {
                push_if_new(entries, &mut seen, parse_entry(&item));
            }
        }
    }
    for entry in legacy {
        push_if_new(entries, &mut seen, entry.clone());
    }
}

fn push_if_new(entries: &mut Vec<StoredAccount>, seen: &mut HashSet<String>, entry: StoredAccount) {
    let email = entry_email(&entry);
    if email.is_empty() {
        let access = entry.token.access_token.clone();
        if !access.is_empty()
            && entries
                .iter()
                .any(|existing| existing.token.access_token == access)
        {
            return;
        }
        entries.push(entry);
        return;
    }
    if seen.insert(email) {
        entries.push(entry);
    }
}

fn emails(entries: &[StoredAccount]) -> HashSet<String> {
    entries
        .iter()
        .map(entry_email)
        .filter(|email| !email.is_empty())
        .collect()
}

fn entries_from_backup(path: &Path) -> Vec<StoredAccount> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Array(items)) => items.iter().map(parse_entry).collect(),
        _ => complete_objects(&bytes).iter().map(parse_entry).collect(),
    }
}

fn read_legacy(dir: &Path) -> Vec<StoredAccount> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for name in ["googleauthtoken.json", "google_auth.json"] {
        let path = dir.join(name);
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let values = match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Array(items)) => items,
            Ok(value) => vec![value],
            Err(_) => complete_objects(&bytes),
        };
        for value in values {
            push_if_new(&mut entries, &mut seen, parse_entry(&value));
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("notor-accounts-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn token(email: &str, disabled: bool, access: &str) -> Value {
        serde_json::json!({
            "token": {
                "access_token": access,
                "token_type": "Bearer",
                "expires_in": "3600",
                "refresh_token": format!("refresh-{email}"),
                "scope": "calendar",
                "expires_at": "10",
                "user": {
                    "id": "1",
                    "email": email,
                    "verified_email": true,
                    "name": email,
                    "given_name": "A",
                    "family_name": "B",
                    "picture": "",
                    "locale": "en"
                },
                "disabled": disabled
            }
        })
    }

    #[test]
    fn clean_file_is_trusted_and_not_rewritten() {
        let dir = dir("clean");
        let path = accounts_path(&dir);
        let body = serde_json::json!([
            token("a@example.com", false, "aaa"),
            token("b@example.com", true, "bbb")
        ]);
        let bytes = serde_json::to_vec(&body).unwrap();
        fs::write(&path, &bytes).unwrap();
        fs::write(
            dir.join("googleauthtoken.json"),
            br#"{"access_token":"old","user":{"email":"ghost@example.com"}}"#,
        )
        .unwrap();
        let store = AccountStore::load(&dir);
        assert_eq!(store.tokens().len(), 2);
        assert!(store.tokens()[1].disabled.unwrap());
        assert_eq!(store.tokens()[0].expires_in, 3600);
        assert_eq!(store.tokens()[0].expires_at, Some(10));
        store.save(&dir).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_file_does_not_resurrect_a_legacy_account() {
        let dir = dir("empty");
        fs::write(accounts_path(&dir), b"[]").unwrap();
        fs::write(
            dir.join("googleauthtoken.json"),
            serde_json::to_vec(&token("ghost@example.com", false, "old")).unwrap(),
        )
        .unwrap();
        let store = AccountStore::load(&dir);
        assert!(store.tokens().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncated_file_keeps_complete_entries_and_fills_from_backup() {
        let dir = dir("trunc");
        let path = accounts_path(&dir);
        let full = serde_json::json!([
            token("a@example.com", false, "aaa"),
            token("b@example.com", true, "bbb")
        ]);
        let bytes = serde_json::to_vec(&full).unwrap();
        fs::write(storage::backup_path(&path), &bytes).unwrap();
        let text = serde_json::to_string(&full).unwrap();
        let cut = text.find(r#""b@example.com""#).unwrap();
        fs::write(&path, text[..cut].as_bytes()).unwrap();
        let store = AccountStore::load(&dir);
        let emails: Vec<_> = store
            .tokens()
            .iter()
            .filter_map(|token| token.user.clone().map(|user| user.email))
            .collect();
        assert_eq!(
            emails,
            vec!["a@example.com".to_string(), "b@example.com".to_string()]
        );
        assert!(store.tokens()[1].disabled.unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_files_and_a_token_without_email_are_kept() {
        let dir = dir("legacy");
        fs::write(
            dir.join("googleauthtoken.json"),
            br#"{"access_token":"solo","token_type":"Bearer","expires_in":1,"refresh_token":"r","scope":"calendar","user":{"email":"solo@example.com","id":"1","verified_email":true,"name":"S","given_name":"S","family_name":"O","picture":""}}"#,
        )
        .unwrap();
        fs::write(
            dir.join("google_auth.json"),
            br#"{"access_token":"anon","token_type":"Bearer","expires_in":1,"refresh_token":"r2","scope":"calendar"}"#,
        )
        .unwrap();
        let store = AccountStore::load(&dir);
        assert_eq!(store.tokens().len(), 2);
        assert!(store.tokens().iter().any(|token| token.user.is_none()));
        assert!(store.tokens().iter().any(|token| token
            .user
            .as_ref()
            .map(|user| user.email.as_str())
            == Some("solo@example.com")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_is_refused_until_loading_finishes() {
        let dir = dir("early");
        let store = AccountStore::loading();
        let err = store.save(&dir).unwrap_err();
        assert!(err.contains("startup"));
        assert!(!accounts_path(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unparsed_entry_is_written_back_unchanged() {
        let dir = dir("raw");
        let raw = serde_json::json!([{"token": "not-an-object", "keep": true}, token("a@example.com", false, "aaa")]);
        fs::write(accounts_path(&dir), serde_json::to_vec(&raw).unwrap()).unwrap();
        let mut store = AccountStore::load(&dir);
        assert_eq!(store.tokens().len(), 1);
        store.dirty = true;
        store.save(&dir).unwrap();
        let written: Value =
            serde_json::from_slice(&fs::read(accounts_path(&dir)).unwrap()).unwrap();
        assert_eq!(written[0]["token"], "not-an-object");
        assert_eq!(written[0]["keep"], true);
        let _ = fs::remove_dir_all(&dir);
    }
}
