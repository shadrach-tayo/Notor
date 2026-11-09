//! Atomic JSON writes. A good file is copied to `.bak` first. A file that
//! cannot be parsed is copied aside and never replaces that backup.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Write `bytes` via a temp file in the same directory, flushed, then renamed.
///
/// When `path` already holds valid JSON, that bytes are copied to `path.bak`
/// before the rename. Invalid JSON and unreadable files are copied to
/// `path.unreadable-<seconds>` and the existing `.bak` is left alone.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    if path.exists() {
        preserve_previous(path)?;
    }
    let tmp = temp_path(path);
    {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".bak");
    path.with_file_name(name)
}

pub fn quarantine_path(path: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let name = format!("{file_name}.unreadable-{stamp}");
    path.with_file_name(name)
}

/// Copy a damaged payload aside. Used when the caller already has the bytes.
pub fn quarantine_bytes(path: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let dest = quarantine_path(path);
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(&dest, bytes)?;
    Ok(dest)
}

fn preserve_previous(path: &Path) -> io::Result<()> {
    match fs::read(path) {
        Ok(existing) if serde_json::from_slice::<serde_json::Value>(&existing).is_ok() => {
            let bak = backup_path(path);
            fs::write(&bak, existing)?;
            Ok(())
        }
        Ok(existing) => {
            let _ = quarantine_bytes(path, &existing);
            Ok(())
        }
        Err(_) => {
            let dest = quarantine_path(path);
            let _ = fs::copy(path, dest);
            Ok(())
        }
    }
}

/// Read a file. `NotFound` stays distinct from a read that failed.
pub fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    fs::read(path)
}

pub fn json_value(bytes: &[u8]) -> Option<serde_json::Value> {
    serde_json::from_slice(bytes).ok()
}

/// Pull every complete top-level JSON object out of a truncated document.
pub fn complete_objects(bytes: &[u8]) -> Vec<serde_json::Value> {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => {
            let valid = &bytes[..err.valid_up_to()];
            match std::str::from_utf8(valid) {
                Ok(text) => text,
                Err(_) => return Vec::new(),
            }
        }
    };
    let mut objects = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '{' {
            index += 1;
            continue;
        }
        if let Some(end) = matching_object_end(&chars, index) {
            let slice: String = chars[index..=end].iter().collect();
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&slice) {
                objects.push(value);
            }
            index = end + 1;
        } else {
            break;
        }
    }
    objects
}

fn matching_object_end(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (offset, ch) in chars[start..].iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match ch {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + offset);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("notor-storage-{name}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir.join("doc.json")
    }

    #[test]
    fn atomic_write_replaces_a_good_file_and_keeps_a_backup() {
        let path = scratch("good");
        atomic_write(&path, br#"{"ok":1}"#).unwrap();
        atomic_write(&path, br#"{"ok":2}"#).unwrap();
        assert_eq!(fs::read(&path).unwrap(), br#"{"ok":2}"#);
        assert_eq!(fs::read(backup_path(&path)).unwrap(), br#"{"ok":1}"#);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn damaged_file_does_not_replace_a_good_backup() {
        let path = scratch("damaged");
        atomic_write(&path, br#"{"ok":1}"#).unwrap();
        atomic_write(&path, br#"{"ok":2}"#).unwrap();
        fs::write(&path, b"{\"ok\":2").unwrap();
        atomic_write(&path, br#"{"ok":3}"#).unwrap();
        assert_eq!(fs::read(&path).unwrap(), br#"{"ok":3}"#);
        // The damaged bytes never become the backup. The backup stays the
        // last good version from before that file was overwritten.
        assert_eq!(fs::read(backup_path(&path)).unwrap(), br#"{"ok":1}"#);
        let parent = path.parent().unwrap();
        let quarantined = fs::read_dir(parent).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".unreadable-")
        });
        assert!(quarantined);
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn truncated_array_keeps_complete_objects_only() {
        let bytes = br#"[{"a":1},{"b":2},{"c":"#;
        let objects = complete_objects(bytes);
        assert_eq!(objects.len(), 2);
        assert_eq!(objects[0]["a"], 1);
        assert_eq!(objects[1]["b"], 2);
    }
}
