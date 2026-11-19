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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_backup: AutoBackup::Daily,
            keep_backups: 10,
            last_backup_at: None,
            last_backup_error: None,
            google_account: None,
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
    pub fn load(path: PathBuf) -> Self {
        let current = match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                log::warn!("settings file is invalid ({e}); using defaults");
                let _ = fs::rename(&path, path.with_extension("json.invalid"));
                Settings::default()
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

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes)?;
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

        // Unknown/missing fields are tolerated (forward/backward compatible).
        fs::write(&path, br#"{"keepBackups": 3, "futureField": true}"#).unwrap();
        let s = SettingsStore::load(path).get();
        assert_eq!((s.keep_backups, s.auto_backup), (3, AutoBackup::Daily));
        let _ = fs::remove_dir_all(&dir);
    }
}
