//! Google Drive backup & restore, built on `tuxnotes-core::backup` and `tuxnotes-gdrive`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::{Mutex as AsyncMutex, oneshot};
use tuxnotes_core::Store;
use tuxnotes_core::backup;
use tuxnotes_gdrive::{DriveClient, LoopbackReceiver, OAuthClient, Pkce, Tokens, random_state};

use crate::AppState;
use crate::credentials::TokenVault;
use crate::settings::{AutoBackup, Settings, SettingsStore};

const DRIVE_FOLDER_NAME: &str = "TuxNotes Backups";
const APP_TAG_KEY: &str = "tuxnotes";
const APP_TAG_VALUE: &str = "backup";
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const SAFETY_PREFIX: &str = "tuxnotes-before-restore-";
const SAFETY_KEEP: usize = 5;
/// Largest backup we're willing to download into memory.
const MAX_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;

/// OAuth client compiled into release builds (set in CI from repository secrets).
const BUILT_IN_CLIENT_ID: Option<&str> = option_env!("TUXNOTES_GOOGLE_CLIENT_ID");
const BUILT_IN_CLIENT_SECRET: Option<&str> = option_env!("TUXNOTES_GOOGLE_CLIENT_SECRET");

pub type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Busy {
    SigningIn,
    BackingUp,
    Restoring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClientSource {
    BuiltIn,
    Custom,
    Environment,
    None,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    pub client_source: ClientSource,
    pub signed_in: bool,
    pub account: Option<String>,
    pub busy: Option<Busy>,
    pub auto_backup: AutoBackup,
    pub keep_backups: u32,
    pub last_backup_at: Option<i64>,
    pub last_backup_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub id: String,
    pub name: String,
    pub created_at: Option<i64>,
    pub size: Option<u64>,
    pub note_count: Option<i64>,
    pub app_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub note_count: i64,
    pub folder_count: i64,
    pub backup_created_at: i64,
    /// Local copy of the notes as they were just before the restore.
    pub safety_copy: String,
}

pub struct BackupService {
    http: reqwest::Client,
    settings: SettingsStore,
    vault: TokenVault,
    data_dir: PathBuf,
    db_path: PathBuf,
    app_version: String,
    /// Cached access token (refresh token lives in the vault).
    session: AsyncMutex<Option<Tokens>>,
    /// Whether a refresh token is stored. Mirrors the vault so status checks don't hit D-Bus.
    signed_in: Mutex<bool>,
    /// Only one sign-in/backup/restore at a time.
    operation: AsyncMutex<()>,
    busy: Mutex<Option<Busy>>,
    cancel_sign_in: Mutex<Option<oneshot::Sender<()>>>,
    folder_id: AsyncMutex<Option<String>>,
}

/// Clears the busy flag when an operation ends, however it ends.
struct BusyGuard<'a>(&'a Mutex<Option<Busy>>);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        *self.0.lock().unwrap() = None;
    }
}

impl BackupService {
    pub fn new(
        data_dir: PathBuf,
        config_dir: PathBuf,
        db_path: PathBuf,
        app_version: String,
    ) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(10 * 60))
            .user_agent(concat!("TuxNotes/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client");
        let vault = TokenVault::new(data_dir.join("google-token"));
        let signed_in = vault.load().is_some();
        Self {
            http,
            settings: SettingsStore::load(config_dir.join("settings.json")),
            vault,
            data_dir,
            db_path,
            app_version,
            session: AsyncMutex::new(None),
            signed_in: Mutex::new(signed_in),
            operation: AsyncMutex::new(()),
            busy: Mutex::new(None),
            cancel_sign_in: Mutex::new(None),
            folder_id: AsyncMutex::new(None),
        }
    }

    fn drive(&self) -> DriveClient {
        DriveClient::new(self.http.clone())
    }

    fn begin(&self, kind: Busy) -> CmdResult<(tokio::sync::MutexGuard<'_, ()>, BusyGuard<'_>)> {
        let lock = self.operation.try_lock().map_err(|_| {
            "Another backup operation is already running. Please wait for it to finish.".to_string()
        })?;
        *self.busy.lock().unwrap() = Some(kind);
        Ok((lock, BusyGuard(&self.busy)))
    }

    // ------------------------------------------------------------ status & prefs

    fn oauth_client(&self, settings: &Settings) -> (ClientSource, Option<OAuthClient>) {
        let non_empty =
            |s: Option<String>| s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        if let Some(id) = non_empty(settings.google_client_id.clone()) {
            let secret = non_empty(settings.google_client_secret.clone());
            return (ClientSource::Custom, Some(OAuthClient::google(id, secret)));
        }
        if let Some(id) = non_empty(std::env::var("TUXNOTES_GOOGLE_CLIENT_ID").ok()) {
            let secret = non_empty(std::env::var("TUXNOTES_GOOGLE_CLIENT_SECRET").ok());
            return (
                ClientSource::Environment,
                Some(OAuthClient::google(id, secret)),
            );
        }
        if let Some(id) = non_empty(BUILT_IN_CLIENT_ID.map(String::from)) {
            let secret = non_empty(BUILT_IN_CLIENT_SECRET.map(String::from));
            return (ClientSource::BuiltIn, Some(OAuthClient::google(id, secret)));
        }
        (ClientSource::None, None)
    }

    pub fn status(&self) -> BackupStatus {
        let s = self.settings.get();
        BackupStatus {
            client_source: self.oauth_client(&s).0,
            signed_in: *self.signed_in.lock().unwrap(),
            account: s.google_account.clone(),
            busy: *self.busy.lock().unwrap(),
            auto_backup: s.auto_backup,
            keep_backups: s.keep_backups,
            last_backup_at: s.last_backup_at,
            last_backup_error: s.last_backup_error.clone(),
        }
    }

    pub fn set_preferences(&self, auto_backup: AutoBackup, keep_backups: u32) -> BackupStatus {
        self.settings.update(|s| {
            s.auto_backup = auto_backup;
            s.keep_backups = keep_backups;
        });
        self.status()
    }

    pub fn set_client(
        &self,
        client_id: Option<String>,
        client_secret: Option<String>,
    ) -> BackupStatus {
        self.settings.update(|s| {
            s.google_client_id = client_id;
            s.google_client_secret = client_secret;
        });
        self.status()
    }

    /// True when an automatic backup should run now.
    pub fn auto_backup_due(&self, now_ms: i64) -> bool {
        let s = self.settings.get();
        let Some(interval) = s.auto_backup.interval_ms() else {
            return false;
        };
        *self.signed_in.lock().unwrap()
            && self.busy.lock().unwrap().is_none()
            && s.last_backup_at
                .is_none_or(|last| now_ms - last >= interval)
    }

    // ------------------------------------------------------------------ account

    pub async fn sign_in(&self, app: &AppHandle) -> CmdResult<BackupStatus> {
        let (_lock, _busy) = self.begin(Busy::SigningIn)?;
        let (_, client) = self.oauth_client(&self.settings.get());
        let client = client.ok_or(
            "Google Drive backup isn't set up in this build. Add your own Google OAuth client \
             under “Advanced” (see docs/google-setup.md).",
        )?;

        let receiver = LoopbackReceiver::bind().await.map_err(err)?;
        let redirect_uri = receiver.redirect_uri();
        let pkce = Pkce::generate();
        let state = random_state();
        let url = client.authorization_url(&redirect_uri, &pkce, &state);
        app.opener()
            .open_url(&url, None::<&str>)
            .map_err(|e| format!("Couldn't open your web browser: {e}"))?;

        let (cancel_tx, cancel_rx) = oneshot::channel();
        *self.cancel_sign_in.lock().unwrap() = Some(cancel_tx);
        let code = tokio::select! {
            result = receiver.wait_for_code(&state) => result.map_err(err)?,
            _ = cancel_rx => return Err("Sign-in cancelled.".into()),
            _ = tokio::time::sleep(SIGN_IN_TIMEOUT) => {
                return Err("Sign-in timed out. Please try again.".into())
            }
        };
        self.cancel_sign_in.lock().unwrap().take();

        let tokens = client
            .exchange_code(&self.http, &code, &pkce, &redirect_uri)
            .await
            .map_err(err)?;
        let refresh = tokens
            .refresh_token
            .clone()
            .expect("checked by exchange_code");
        let vault = self.vault.clone();
        tauri::async_runtime::spawn_blocking(move || vault.save(&refresh))
            .await
            .map_err(err)?
            .map_err(|e| format!("Couldn't store the Google sign-in securely: {e}"))?;

        let account = self
            .drive()
            .about(&tokens.access_token)
            .await
            .ok()
            .map(|u| u.email_address)
            .filter(|e| !e.is_empty());
        *self.session.lock().await = Some(tokens);
        *self.folder_id.lock().await = None;
        *self.signed_in.lock().unwrap() = true;
        self.settings.update(|s| {
            s.google_account = account;
            s.last_backup_error = None;
        });
        log::info!("signed in to Google Drive");
        Ok(self.status())
    }

    pub fn cancel_sign_in(&self) {
        if let Some(tx) = self.cancel_sign_in.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }

    pub async fn sign_out(&self) -> CmdResult<BackupStatus> {
        let (_lock, _busy) = self.begin(Busy::SigningIn)?;
        let vault = self.vault.clone();
        let refresh = tauri::async_runtime::spawn_blocking(move || vault.load())
            .await
            .map_err(err)?;
        // Best effort: forget the account locally even if revoking fails (e.g. offline).
        if let (Some(token), (_, Some(client))) = (refresh, self.oauth_client(&self.settings.get()))
            && let Err(e) = client.revoke(&self.http, &token).await
        {
            log::warn!("could not revoke Google token: {e}");
        }
        self.forget_account().await;
        Ok(self.status())
    }

    async fn forget_account(&self) {
        let vault = self.vault.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || vault.clear()).await;
        *self.session.lock().await = None;
        *self.folder_id.lock().await = None;
        *self.signed_in.lock().unwrap() = false;
        self.settings.update(|s| s.google_account = None);
    }

    /// A valid access token, refreshing it if needed.
    async fn access_token(&self, force_refresh: bool) -> CmdResult<String> {
        let mut session = self.session.lock().await;
        if let Some(tokens) = session
            .as_ref()
            .filter(|t| !force_refresh && !t.needs_refresh())
        {
            return Ok(tokens.access_token.clone());
        }
        let vault = self.vault.clone();
        let refresh = tauri::async_runtime::spawn_blocking(move || vault.load())
            .await
            .map_err(err)?
            .ok_or("You're not signed in to Google Drive.")?;
        let (_, client) = self.oauth_client(&self.settings.get());
        let client = client.ok_or("Google Drive backup isn't set up in this build.")?;
        match client.refresh(&self.http, &refresh).await {
            Ok(tokens) => {
                let access = tokens.access_token.clone();
                *session = Some(tokens);
                Ok(access)
            }
            Err(tuxnotes_gdrive::Error::SessionExpired) => {
                drop(session);
                self.forget_account().await;
                Err(tuxnotes_gdrive::Error::SessionExpired.to_string())
            }
            Err(e) => Err(err(e)),
        }
    }

    /// Run a Drive call, refreshing the access token once if Google rejects it.
    async fn with_token<T, F, Fut>(&self, call: F) -> CmdResult<T>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = tuxnotes_gdrive::Result<T>>,
    {
        let token = self.access_token(false).await?;
        match call(token).await {
            Err(tuxnotes_gdrive::Error::Unauthorized) => {
                let token = self.access_token(true).await?;
                call(token).await.map_err(err)
            }
            other => other.map_err(err),
        }
    }

    async fn backup_folder(&self) -> CmdResult<String> {
        let mut cached = self.folder_id.lock().await;
        if let Some(id) = cached.as_ref() {
            return Ok(id.clone());
        }
        let drive = self.drive();
        let id = self
            .with_token(|t| {
                let drive = drive.clone();
                async move { drive.find_or_create_folder(&t, DRIVE_FOLDER_NAME).await }
            })
            .await?;
        *cached = Some(id.clone());
        Ok(id)
    }

    // ------------------------------------------------------------------ backups

    pub async fn backup_now(&self, app: &AppHandle) -> CmdResult<BackupInfo> {
        let (_lock, _busy) = self.begin(Busy::BackingUp)?;
        let result = self.run_backup(app).await;
        let now = now_ms();
        self.settings.update(|s| match &result {
            Ok(_) => {
                s.last_backup_at = Some(now);
                s.last_backup_error = None;
            }
            Err(e) => s.last_backup_error = Some(e.clone()),
        });
        result
    }

    async fn run_backup(&self, app: &AppHandle) -> CmdResult<BackupInfo> {
        let tmp = self.tmp_dir()?;
        let archive = tmp.join(backup::backup_file_name(now_ms()));
        let _cleanup = RemoveOnDrop(vec![archive.clone()]);

        let app2 = app.clone();
        let archive2 = archive.clone();
        let version = self.app_version.clone();
        let manifest = tauri::async_runtime::spawn_blocking(move || {
            let state = app2.state::<AppState>();
            let store = state
                .store
                .lock()
                .map_err(|_| "note store unavailable".to_string())?;
            backup::create_backup(&store, &archive2, &version).map_err(err)
        })
        .await
        .map_err(err)??;
        let bytes = fs::read(&archive).map_err(err)?;
        let name = archive.file_name().unwrap().to_string_lossy().into_owned();

        let props = BTreeMap::from([
            (APP_TAG_KEY.to_string(), APP_TAG_VALUE.to_string()),
            ("formatVersion".into(), manifest.format_version.to_string()),
            ("createdAt".into(), manifest.created_at.to_string()),
            ("noteCount".into(), manifest.note_count.to_string()),
            ("appVersion".into(), manifest.app_version.clone()),
        ]);
        let folder = self.backup_folder().await?;
        let drive = self.drive();
        let file = self
            .with_token(|t| {
                let (drive, folder, name, bytes, props) = (
                    drive.clone(),
                    folder.clone(),
                    name.clone(),
                    bytes.clone(),
                    props.clone(),
                );
                async move {
                    drive
                        .upload(&t, &folder, &name, "application/zip", bytes, &props)
                        .await
                }
            })
            .await?;
        log::info!("uploaded backup {} ({} bytes)", file.name, bytes.len());

        if let Err(e) = self.prune_remote(&folder).await {
            log::warn!("could not remove old backups: {e}");
        }
        Ok(BackupInfo {
            id: file.id,
            name: file.name,
            created_at: Some(manifest.created_at),
            size: file.size.or(Some(bytes.len() as u64)),
            note_count: Some(manifest.note_count),
            app_version: Some(manifest.app_version),
        })
    }

    async fn prune_remote(&self, folder: &str) -> CmdResult<()> {
        let keep = self.settings.get().keep_backups as usize;
        let mut backups = self.list_in(folder).await?;
        // Newest first; drop everything after the first `keep`.
        backups.sort_by_key(|b| std::cmp::Reverse(b.created_at.unwrap_or(0)));
        let drive = self.drive();
        for old in backups.into_iter().skip(keep) {
            self.with_token(|t| {
                let (drive, id) = (drive.clone(), old.id.clone());
                async move { drive.delete(&t, &id).await }
            })
            .await?;
            log::info!("removed old backup {}", old.name);
        }
        Ok(())
    }

    pub async fn list_backups(&self) -> CmdResult<Vec<BackupInfo>> {
        let folder = self.backup_folder().await?;
        let mut list = self.list_in(&folder).await?;
        list.sort_by_key(|b| std::cmp::Reverse(b.created_at.unwrap_or(0)));
        Ok(list)
    }

    async fn list_in(&self, folder: &str) -> CmdResult<Vec<BackupInfo>> {
        let drive = self.drive();
        let files = self
            .with_token(|t| {
                let (drive, folder) = (drive.clone(), folder.to_string());
                async move {
                    drive
                        .list_files(&t, &folder, APP_TAG_KEY, APP_TAG_VALUE)
                        .await
                }
            })
            .await?;
        Ok(files
            .into_iter()
            .map(|f| BackupInfo {
                created_at: f
                    .app_properties
                    .get("createdAt")
                    .and_then(|v| v.parse().ok()),
                note_count: f
                    .app_properties
                    .get("noteCount")
                    .and_then(|v| v.parse().ok()),
                app_version: f.app_properties.get("appVersion").cloned(),
                id: f.id,
                name: f.name,
                size: f.size,
            })
            .collect())
    }

    // ------------------------------------------------------------------ restore

    pub async fn restore(&self, app: &AppHandle, file_id: &str) -> CmdResult<RestoreResult> {
        let (_lock, _busy) = self.begin(Busy::Restoring)?;
        let drive = self.drive();
        let bytes = self
            .with_token(|t| {
                let (drive, id) = (drive.clone(), file_id.to_string());
                async move { drive.download(&t, &id, MAX_DOWNLOAD_BYTES).await }
            })
            .await?;

        let tmp = self.tmp_dir()?;
        let stamp = now_ms();
        let archive = tmp.join(format!("restore-{stamp}.zip"));
        let staged_db = tmp.join(format!("restore-{stamp}.db"));
        let mut cleanup = vec![archive.clone()];
        cleanup.extend(backup::db_with_sidecars(&staged_db));
        let _cleanup = RemoveOnDrop(cleanup);
        fs::write(&archive, &bytes).map_err(err)?;

        let app2 = app.clone();
        let db_path = self.db_path.clone();
        let safety_dir = self.data_dir.join("safety");
        let version = self.app_version.clone();
        tauri::async_runtime::spawn_blocking(move || {
            // 1. Fully validate the backup into a separate file.
            let manifest = backup::extract_backup(&archive, &staged_db).map_err(err)?;
            let state = app2.state::<AppState>();
            let mut store = state
                .store
                .lock()
                .map_err(|_| "note store unavailable".to_string())?;
            // 2. Keep a local copy of the current notes, just in case.
            fs::create_dir_all(&safety_dir).map_err(err)?;
            let safety = safety_dir.join(format!(
                "{SAFETY_PREFIX}{}",
                backup::backup_file_name(now_ms()).trim_start_matches("tuxnotes-backup-")
            ));
            backup::create_backup(&store, &safety, &version).map_err(err)?;
            let _ = backup::prune_local_backups(&safety_dir, SAFETY_PREFIX, SAFETY_KEEP);
            // 3. Swap the database files while no connection is open.
            swap_database(&mut store, &db_path, &staged_db)?;
            Ok::<_, String>(RestoreResult {
                note_count: manifest.note_count,
                folder_count: manifest.folder_count,
                backup_created_at: manifest.created_at,
                safety_copy: safety.display().to_string(),
            })
        })
        .await
        .map_err(err)?
    }

    fn tmp_dir(&self) -> CmdResult<PathBuf> {
        let dir = self.data_dir.join("tmp");
        fs::create_dir_all(&dir).map_err(err)?;
        Ok(dir)
    }

    /// Remove leftovers from interrupted operations (called at startup).
    pub fn clean_tmp(&self) {
        let _ = fs::remove_dir_all(self.data_dir.join("tmp"));
    }
}

/// Replace the live database with `staged` (already validated), keeping the
/// store usable on every failure path.
fn swap_database(store: &mut Store, live: &Path, staged: &Path) -> CmdResult<()> {
    // Close the live connection (an in-memory placeholder takes its place).
    let placeholder = Store::open_in_memory().map_err(err)?;
    drop(std::mem::replace(store, placeholder));

    for sidecar in backup::db_with_sidecars(live).iter().skip(1) {
        let _ = fs::remove_file(sidecar);
    }
    if let Err(e) = fs::rename(staged, live) {
        *store = Store::open(live).map_err(err)?;
        return Err(format!("Couldn't replace the notes database: {e}"));
    }
    *store = Store::open(live).map_err(err)?;
    log::info!("restored notes database from backup");
    Ok(())
}

struct RemoveOnDrop(Vec<PathBuf>);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = fs::remove_file(p);
        }
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn swap_replaces_live_database_and_keeps_store_usable() {
        let dir = std::env::temp_dir().join(format!("tuxnotes-swap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let live = dir.join("notes.db");
        let staged = dir.join("staged.db");

        let mut store = Store::open(&live).unwrap();
        let n = store.create_note(None).unwrap();
        store
            .update_note_content(&n.id, &json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"old"}]}]}))
            .unwrap();
        {
            let other = Store::open(&staged).unwrap();
            let m = other.create_note(None).unwrap();
            other
                .update_note_content(&m.id, &json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"restored"}]}]}))
                .unwrap();
        }
        swap_database(&mut store, &live, &staged).unwrap();
        assert!(!staged.exists());
        assert_eq!(store.search("restored", 10).unwrap().len(), 1);
        assert!(store.search("old", 10).unwrap().is_empty());
        // Persisted on disk, not just in memory.
        drop(store);
        assert_eq!(
            Store::open(&live)
                .unwrap()
                .search("restored", 10)
                .unwrap()
                .len(),
            1
        );

        // A failed swap (missing staged file) leaves the live database open and intact.
        let mut store = Store::open(&live).unwrap();
        assert!(swap_database(&mut store, &live, &dir.join("missing.db")).is_err());
        assert_eq!(store.search("restored", 10).unwrap().len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
