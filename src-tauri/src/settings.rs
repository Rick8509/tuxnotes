//! User preferences, stored as JSON in the XDG config directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AutoBackup {
    Off,
    #[default]
    Daily,
    Weekly,
}

impl AutoBackup {
    /// Minimum time between automatic backups, in milliseconds.
    pub fn interval_ms(self) -> Option<i64> {
        const DAY: i64 = 24 * 60 * 60 * 1000;
        match self {
            AutoBackup::Off => None,
            AutoBackup::Daily => Some(DAY),
            AutoBackup::Weekly => Some(7 * DAY),
        }
    }
}

/// How backups are encrypted: the salt and Argon2id costs the backup key was derived
/// with. The key itself is in the keyring; the password is never stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEncryption {
    /// Hex.
    pub salt: String,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub auto_backup: AutoBackup,
    /// How many backups to keep on Google Drive (older ones are deleted).
    pub keep_backups: u32,
    pub last_backup_at: Option<i64>,
    pub last_backup_error: Option<String>,
    /// Email of the connected Google account, for display only.
    pub google_account: Option<String>,
    /// Set when backups are encrypted with a password.
    pub backup_encryption: Option<BackupEncryption>,
    /// Back up (if anything changed) when TuxNotes is closed.
    pub backup_on_close: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_backup: AutoBackup::Daily,
            keep_backups: 10,
            last_backup_at: None,
            last_backup_error: None,
            google_account: None,
            backup_encryption: None,
            backup_on_close: true,
        }
    }
}

pub const MIN_KEEP: u32 = 1;
pub const MAX_KEEP: u32 = 100;

pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<Settings>,
}

impl SettingsStore {
    /// Load settings, falling back to defaults if the file is missing or unreadable.
    /// A single bad value (e.g. written by a newer version) only resets that value.
    pub fn load(path: PathBuf) -> Self {
        let current = match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                log::warn!("settings file is invalid ({e}); keeping the values that can be read");
                let _ = fs::copy(&path, path.with_extension("json.invalid"));
                parse_leniently(&raw)
            }),
            Err(_) => Settings::default(),
        };
        Self {
            path,
            current: Mutex::new(current),
        }
    }

    pub fn get(&self) -> Settings {
        self.current.lock().expect("settings lock").clone()
    }

    /// Change settings and save them. Saving failures are logged, not fatal.
    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> Settings {
        let mut guard = self.current.lock().expect("settings lock");
        change(&mut guard);
        guard.keep_backups = guard.keep_backups.clamp(MIN_KEEP, MAX_KEEP);
        if let Err(e) = write_atomic(&self.path, &serde_json::to_vec_pretty(&*guard).unwrap()) {
            log::error!("could not save settings to {}: {e}", self.path.display());
        }
        guard.clone()
    }
}

/// Settings field by field: any value that can't be read gets its default, the rest
/// are kept. Unreadable JSON gives all defaults.
fn parse_leniently(raw: &str) -> Settings {
    let mut s = Settings::default();
    let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(raw) else {
        return s;
    };
    fn field<T: serde::de::DeserializeOwned>(
        obj: &serde_json::Map<String, serde_json::Value>,
        key: &str,
        target: &mut T,
    ) {
        if let Some(value) = obj.get(key)
            && let Ok(v) = serde_json::from_value(value.clone())
        {
            *target = v;
        }
    }
    field(&obj, "autoBackup", &mut s.auto_backup);
    field(&obj, "keepBackups", &mut s.keep_backups);
    field(&obj, "lastBackupAt", &mut s.last_backup_at);
    field(&obj, "lastBackupError", &mut s.last_backup_error);
    field(&obj, "googleAccount", &mut s.google_account);
    field(&obj, "backupEncryption", &mut s.backup_encryption);
    field(&obj, "backupOnClose", &mut s.backup_on_close);
    s
}

/// Write a new file next to the old one, flush it to disk, then swap it in, so a crash
/// leaves either the old or the new settings, never a half-written file.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_corrupt_files_give_defaults_and_updates_persist() {
        let dir = std::env::temp_dir().join(format!("tuxnotes-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let _ = fs::remove_dir_all(&dir);

        let s = SettingsStore::load(path.clone());
        assert_eq!(s.get(), Settings::default());

        s.update(|s| {
            s.auto_backup = AutoBackup::Weekly;
            s.keep_backups = 1000; // clamped
        });
        let reloaded = SettingsStore::load(path.clone()).get();
        assert_eq!(reloaded.auto_backup, AutoBackup::Weekly);
        assert_eq!(reloaded.keep_backups, MAX_KEEP);

        fs::write(&path, b"{ not json").unwrap();
        assert_eq!(SettingsStore::load(path.clone()).get(), Settings::default());
        assert!(path.with_extension("json.invalid").exists());

        // One unreadable value (e.g. from a newer version) only resets that value.
        fs::write(
            &path,
            br#"{"autoBackup": "hourly", "keepBackups": 7, "backupOnClose": false,
                "backupEncryption": {"salt": "00ff", "mKib": 65536, "t": 3, "p": 1}}"#,
        )
        .unwrap();
        let s = SettingsStore::load(path.clone()).get();
        assert_eq!(s.auto_backup, AutoBackup::Daily, "unknown value → default");
        assert_eq!((s.keep_backups, s.backup_on_close), (7, false));
        assert_eq!(
            s.backup_encryption.map(|e| e.salt).as_deref(),
            Some("00ff"),
            "encryption kept"
        );

        // Unknown/missing fields are tolerated (forward/backward compatible).
        fs::write(&path, br#"{"keepBackups": 3, "futureField": true}"#).unwrap();
        let s = SettingsStore::load(path).get();
        assert_eq!((s.keep_backups, s.auto_backup), (3, AutoBackup::Daily));
        let _ = fs::remove_dir_all(&dir);
    }
}
