//! Google Drive backup & restore, built on `tuxnotes-core::backup` and `tuxnotes-gdrive`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::{Mutex as AsyncMutex, oneshot};
use tuxnotes_core::Store;
use tuxnotes_core::backup;
use tuxnotes_core::crypto::{KdfParams, SALT_LEN};
use tuxnotes_core::encrypted_backup::{self, BackupKey};
use tuxnotes_gdrive::{DriveClient, LoopbackReceiver, OAuthClient, Pkce, Tokens, random_state};

use crate::AppState;
use crate::credentials::{BACKUP_KEY_ACCOUNT, TokenVault};
use crate::google_client::{self, ClientSource};
use crate::settings::{AutoBackup, BackupEncryption, SettingsStore};

/// Event carrying the Google sign-in URL, so the UI can offer "Copy link"
/// if the browser didn't open.
pub const SIGN_IN_URL_EVENT: &str = "google-sign-in-url";

const DRIVE_FOLDER_NAME: &str = "TuxNotes Backups";
const APP_TAG_KEY: &str = "tuxnotes";
const APP_TAG_VALUE: &str = "backup";
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const SAFETY_PREFIX: &str = "tuxnotes-before-restore-";
const SAFETY_KEEP: usize = 5;
/// Largest backup we're willing to download (it streams to disk, not into memory).
const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024 * 1024;
/// Returned by `restore` when an encrypted backup needs its password; the UI asks for it.
pub const PASSWORD_NEEDED: &str = "BACKUP_PASSWORD_NEEDED";

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
    /// New backups are encrypted with a password.
    pub encrypted: bool,
    pub backup_on_close: bool,
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
    pub encrypted: bool,
    /// Encrypted with a password this computer doesn't have the key for.
    pub needs_password: bool,
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
    /// The key derived from the backup password, when backups are encrypted.
    key_vault: TokenVault,
    /// `Store::change_count` when the last backup was made (or TuxNotes started), to
    /// tell whether there's anything new to back up when closing.
    change_baseline: Mutex<u64>,
    data_dir: PathBuf,
    config_dir: PathBuf,
    db_path: PathBuf,
    attachments_dir: PathBuf,
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
            // Fail when a transfer stalls, not after a fixed time: big backups on slow
            // connections take as long as they take.
            .read_timeout(Duration::from_secs(120))
            .user_agent(concat!("TuxNotes/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client");
        let vault = TokenVault::new(data_dir.join("google-token"));
        let signed_in = vault.load().is_some();
        Self {
            http,
            settings: SettingsStore::load(config_dir.join("settings.json")),
            vault,
            key_vault: TokenVault::with_account(BACKUP_KEY_ACCOUNT, data_dir.join("backup-key")),
            change_baseline: Mutex::new(0),
            attachments_dir: data_dir.join("attachments"),
            data_dir,
            config_dir,
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

    fn oauth_client(&self) -> (ClientSource, Option<OAuthClient>) {
        google_client::resolve(&self.config_dir)
    }

    pub fn status(&self) -> BackupStatus {
        let s = self.settings.get();
        BackupStatus {
            client_source: self.oauth_client().0,
            signed_in: *self.signed_in.lock().unwrap(),
            account: s.google_account.clone(),
            busy: *self.busy.lock().unwrap(),
            auto_backup: s.auto_backup,
            keep_backups: s.keep_backups,
            last_backup_at: s.last_backup_at,
            last_backup_error: s.last_backup_error.clone(),
            encrypted: s.backup_encryption.is_some(),
            backup_on_close: s.backup_on_close,
        }
    }

    pub fn set_preferences(
        &self,
        auto_backup: AutoBackup,
        keep_backups: u32,
        backup_on_close: bool,
    ) -> BackupStatus {
        self.settings.update(|s| {
            s.auto_backup = auto_backup;
            s.keep_backups = keep_backups;
            s.backup_on_close = backup_on_close;
        });
        self.status()
    }

    // --------------------------------------------------------------- encryption

    /// Encrypt new backups with `password` (also used to change it), or stop encrypting
    /// them with `None`. Backups already on Google Drive keep the password they were made with.
    pub async fn set_backup_password(&self, password: Option<String>) -> CmdResult<BackupStatus> {
        // Not in the middle of a backup that's using the current key.
        let _wait = self.operation.lock().await;
        let vault = self.key_vault.clone();
        match password {
            None => {
                tauri::async_runtime::spawn_blocking(move || vault.clear())
                    .await
                    .map_err(err)?;
                self.settings.update(|s| s.backup_encryption = None);
            }
            Some(password) => {
                let encryption = tauri::async_runtime::spawn_blocking(move || {
                    let key = BackupKey::create(&password).map_err(err)?;
                    vault.save(&to_hex(key.key_bytes())).map_err(err)?;
                    Ok::<_, String>(encryption_settings(&key))
                })
                .await
                .map_err(err)??;
                self.settings
                    .update(|s| s.backup_encryption = Some(encryption));
            }
        }
        Ok(self.status())
    }

    // ------------------------------------------------------- backing up on close

    /// Remember the store's change counter as "backed up" (at startup and after
    /// each backup or restore).
    pub fn set_change_baseline(&self, changes: u64) {
        *self.change_baseline.lock().unwrap() = changes;
    }

    /// Whether closing TuxNotes should make a backup first: the setting is on, Google
    /// Drive is connected, and something changed since the last backup (or it failed).
    pub fn backup_needed_on_close(&self, store: &Store) -> bool {
        let s = self.settings.get();
        if !s.backup_on_close || !*self.signed_in.lock().unwrap() {
            return false;
        }
        let edited_since = match (store.last_change_at().ok().flatten(), s.last_backup_at) {
            (Some(changed), Some(backed_up)) => changed > backed_up,
            (Some(_), None) => true,
            (None, _) => false,
        };
        store.change_count() > *self.change_baseline.lock().unwrap()
            || edited_since
            || s.last_backup_error.is_some()
    }

    /// The backup made while TuxNotes closes (its window is already hidden).
    /// Returns whether a backup was made.
    pub async fn backup_before_exit(&self, app: &AppHandle) -> CmdResult<bool> {
        // Let a backup or restore that's already running finish first.
        drop(self.operation.lock().await);
        let needed = {
            let state = app.state::<AppState>();
            let store = state
                .store
                .lock()
                .map_err(|_| "note store unavailable".to_string())?;
            self.backup_needed_on_close(&store)
        };
        if !needed {
            return Ok(false);
        }
        log::info!("backing up before closing");
        self.backup_now(app).await.map(|_| true)
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
        let (_, client) = self.oauth_client();
        let client = client.ok_or(
            "Google Drive backup isn't available in this copy of TuxNotes \
             (no Google sign-in client is configured).",
        )?;

        let receiver = LoopbackReceiver::bind().await.map_err(err)?;
        let redirect_uri = receiver.redirect_uri();
        let pkce = Pkce::generate();
        let state = random_state();
        let url = client.authorization_url(&redirect_uri, &pkce, &state);
        // Give the UI the link first, so "Copy link" works even if no browser opens.
        let _ = app.emit(SIGN_IN_URL_EVENT, &url);
        if let Err(e) = app.opener().open_url(&url, None::<&str>) {
            log::warn!(
                "couldn't open the web browser ({e}); waiting for the user to open the link"
            );
        }

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
        if let (Some(token), (_, Some(client))) = (refresh, self.oauth_client())
            && let Err(e) = client.revoke(&self.http, &token).await
        {
            log::warn!("could not revoke Google token: {e}");
        }
        self.forget_account().await;
        // Signing out on purpose: an old "backup failed" message no longer applies.
        // (When the sign-in expires instead, that message stays to explain why.)
        self.settings.update(|s| s.last_backup_error = None);
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
        let (_, client) = self.oauth_client();
        let client =
            client.ok_or("Google Drive backup isn't available in this copy of TuxNotes.")?;
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
        // Never hold the cache lock while talking to Google: when the sign-in turns out
        // to have expired, `forget_account` clears this cache too and would wait forever.
        let cached = self.folder_id.lock().await.clone();
        if let Some(id) = cached {
            return Ok(id);
        }
        let drive = self.drive();
        let id = self
            .with_token(|t| {
                let drive = drive.clone();
                async move { drive.find_or_create_folder(&t, DRIVE_FOLDER_NAME).await }
            })
            .await?;
        *self.folder_id.lock().await = Some(id.clone());
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
        let sealed = archive.with_extension("zip.enc");
        let _cleanup = RemoveOnDrop(vec![archive.clone(), sealed.clone()]);
        let encryption = self.settings.get().backup_encryption;
        // Fail closed: a saved backup key means backups are meant to be encrypted, even
        // if the settings say otherwise (e.g. the settings file was lost or damaged).
        if encryption.is_none() {
            let vault = self.key_vault.clone();
            let key_saved = tauri::async_runtime::spawn_blocking(move || vault.load().is_some())
                .await
                .map_err(err)?;
            check_encryption_settings(encryption.as_ref(), key_saved)?;
        }

        let app2 = app.clone();
        let archive2 = archive.clone();
        let attachments = self.attachments_dir.clone();
        let version = self.app_version.clone();
        let (manifest, changes) = tauri::async_runtime::spawn_blocking(move || {
            let state = app2.state::<AppState>();
            let store = state
                .store
                .lock()
                .map_err(|_| "note store unavailable".to_string())?;
            let manifest = backup::create_backup(&store, Some(&attachments), &archive2, &version)
                .map_err(err)?;
            Ok::<_, String>((manifest, store.change_count()))
        })
        .await
        .map_err(err)??;

        // Encrypt before anything leaves the computer. If the key is missing, fail
        // rather than upload unencrypted.
        let upload = match &encryption {
            None => archive.clone(),
            Some(enc) => {
                let (vault, enc, src, dest) = (
                    self.key_vault.clone(),
                    enc.clone(),
                    archive.clone(),
                    sealed.clone(),
                );
                tauri::async_runtime::spawn_blocking(move || {
                    let key = load_backup_key(&vault, &enc)?;
                    encrypted_backup::encrypt_file(&src, &dest, &key).map_err(err)
                })
                .await
                .map_err(err)??;
                sealed.clone()
            }
        };
        let size = fs::metadata(&upload).map_err(err)?.len();
        let name = upload.file_name().unwrap().to_string_lossy().into_owned();

        let mut props = BTreeMap::from([
            (APP_TAG_KEY.to_string(), APP_TAG_VALUE.to_string()),
            ("formatVersion".into(), manifest.format_version.to_string()),
            ("createdAt".into(), manifest.created_at.to_string()),
            ("appVersion".into(), manifest.app_version.clone()),
        ]);
        match &encryption {
            // Encrypted backups don't reveal even how many notes they hold.
            Some(enc) => {
                props.insert("encrypted".into(), "1".into());
                props.insert("keySalt".into(), enc.salt.clone());
            }
            None => {
                props.insert("noteCount".into(), manifest.note_count.to_string());
            }
        }
        let mime = if encryption.is_some() {
            "application/octet-stream"
        } else {
            "application/zip"
        };
        let folder = self.backup_folder().await?;
        let drive = self.drive();
        let file = self
            .with_token(|t| {
                let (drive, folder, name, path, props) = (
                    drive.clone(),
                    folder.clone(),
                    name.clone(),
                    upload.clone(),
                    props.clone(),
                );
                async move {
                    drive
                        .upload_file(&t, &folder, &name, mime, &path, &props)
                        .await
                }
            })
            .await?;
        log::info!("uploaded backup {} ({size} bytes)", file.name);
        self.set_change_baseline(changes);

        if let Err(e) = self.prune_remote(&folder).await {
            log::warn!("could not remove old backups: {e}");
        }
        Ok(BackupInfo {
            id: file.id,
            name: file.name,
            created_at: Some(manifest.created_at),
            size: file.size.or(Some(size)),
            note_count: Some(manifest.note_count),
            app_version: Some(manifest.app_version),
            encrypted: encryption.is_some(),
            needs_password: false,
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
        let local_salt = self.settings.get().backup_encryption.map(|e| e.salt);
        Ok(files
            .into_iter()
            .map(|f| BackupInfo {
                encrypted: f.app_properties.get("encrypted").is_some_and(|v| v == "1"),
                needs_password: f.app_properties.get("encrypted").is_some_and(|v| v == "1")
                    && (local_salt.is_none()
                        || f.app_properties.get("keySalt") != local_salt.as_ref()),
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

    /// Restore a backup. Encrypted backups are opened with `password`, or without one
    /// when this computer has the key for them; otherwise this fails with [`PASSWORD_NEEDED`].
    pub async fn restore(
        &self,
        app: &AppHandle,
        file_id: &str,
        password: Option<String>,
    ) -> CmdResult<RestoreResult> {
        let (_lock, _busy) = self.begin(Busy::Restoring)?;
        // What TuxNotes recorded about this backup when uploading it (Drive keeps these
        // properties private to TuxNotes). An encrypted backup must still be encrypted.
        let folder = self.backup_folder().await?;
        let expect_encrypted = self
            .list_in(&folder)
            .await?
            .into_iter()
            .find(|b| b.id == file_id)
            .ok_or("This backup wasn't found in the TuxNotes Backups folder.")?
            .encrypted;
        let tmp = self.tmp_dir()?;
        let stamp = now_ms();
        let download = tmp.join(format!("restore-{stamp}.download"));
        let archive = tmp.join(format!("restore-{stamp}.zip"));
        let staged_db = tmp.join(format!("restore-{stamp}.db"));
        let staged_attachments = tmp.join(format!("restore-{stamp}-attachments"));
        let mut cleanup = vec![
            download.clone(),
            archive.clone(),
            staged_attachments.clone(),
        ];
        cleanup.extend(backup::db_with_sidecars(&staged_db));
        let _cleanup = RemoveOnDrop(cleanup);
        let drive = self.drive();
        self.with_token(|t| {
            let (drive, id, dest) = (drive.clone(), file_id.to_string(), download.clone());
            async move {
                drive
                    .download_to_file(&t, &id, &dest, MAX_DOWNLOAD_BYTES)
                    .await
            }
        })
        .await?;

        // Decrypt first (if needed), before touching anything.
        let encryption = self.settings.get().backup_encryption;
        let (vault, download2, archive2) =
            (self.key_vault.clone(), download.clone(), archive.clone());
        let adopt = tauri::async_runtime::spawn_blocking(move || {
            let Some(header) = encrypted_backup::read_header(&download2).map_err(err)? else {
                if expect_encrypted {
                    return Err(
                        "This backup should be encrypted but isn't. It may have been \
                                changed by someone else, so it wasn't restored."
                            .to_string(),
                    );
                }
                fs::rename(&download2, &archive2).map_err(err)?;
                return Ok::<_, String>(None);
            };
            let typed = password.is_some();
            let key = match password {
                Some(password) => {
                    BackupKey::derive(&password, header.params, header.salt).map_err(err)?
                }
                None => encryption
                    .as_ref()
                    .and_then(|enc| load_backup_key(&vault, enc).ok())
                    .filter(|key| key.fits(&header))
                    .ok_or_else(|| PASSWORD_NEEDED.to_string())?,
            };
            encrypted_backup::decrypt_file(&download2, &archive2, &key).map_err(err)?;
            // Restoring an encrypted backup on a computer that doesn't encrypt yet:
            // keep encrypting with the same password from now on.
            Ok((typed && encryption.is_none()).then_some(key))
        })
        .await
        .map_err(err)??;

        let app2 = app.clone();
        let db_path = self.db_path.clone();
        let attachments = self.attachments_dir.clone();
        let safety_dir = self.data_dir.join("safety");
        let version = self.app_version.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            // 1. Fully validate the backup (database and attachments) into separate files.
            let manifest = backup::extract_backup(&archive, &staged_db, Some(&staged_attachments))
                .map_err(err)?;
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
            backup::create_backup(&store, Some(&attachments), &safety, &version).map_err(err)?;
            let _ = backup::prune_local_backups(&safety_dir, SAFETY_PREFIX, SAFETY_KEEP);
            // 3. Swap the database and attachments while no connection is open.
            swap_database(
                &mut store,
                &db_path,
                &staged_db,
                &attachments,
                &staged_attachments,
            )?;
            let result = RestoreResult {
                note_count: manifest.note_count,
                folder_count: manifest.folder_count,
                backup_created_at: manifest.created_at,
                safety_copy: safety.display().to_string(),
            };
            Ok::<_, String>((result, store.change_count()))
        })
        .await
        .map_err(err)??;
        let (result, changes) = result;
        // The notes now match a backup: nothing new to back up.
        self.set_change_baseline(changes);

        if let Some(key) = adopt {
            let vault = self.key_vault.clone();
            let encryption = encryption_settings(&key);
            let saved =
                tauri::async_runtime::spawn_blocking(move || vault.save(&to_hex(key.key_bytes())))
                    .await
                    .map_err(err)?;
            match saved {
                Ok(()) => {
                    self.settings
                        .update(|s| s.backup_encryption = Some(encryption));
                }
                Err(e) => log::warn!("could not save the backup key: {e}"),
            }
        }
        Ok(result)
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

/// Refuse to upload unencrypted while a backup key is saved: the encryption settings
/// must have gone missing, and uploading would silently drop the protection.
fn check_encryption_settings(
    encryption: Option<&BackupEncryption>,
    key_saved: bool,
) -> CmdResult<()> {
    if encryption.is_none() && key_saved {
        return Err(
            "Backups are meant to be encrypted, but this computer's backup settings \
                    are missing or damaged, so nothing was uploaded. Set the backup password \
                    again in Settings → Backup."
                .into(),
        );
    }
    Ok(())
}

fn encryption_settings(key: &BackupKey) -> BackupEncryption {
    BackupEncryption {
        salt: to_hex(&key.salt),
        m_kib: key.params.m_kib,
        t: key.params.t,
        p: key.params.p,
    }
}

/// The saved backup key (blocking: reads the keyring).
fn load_backup_key(vault: &TokenVault, enc: &BackupEncryption) -> CmdResult<BackupKey> {
    let missing = || {
        "The backup password isn't saved on this computer any more. \
         Set it again in Settings → Backup."
            .to_string()
    };
    let key = from_hex(&vault.load().ok_or_else(missing)?).ok_or_else(missing)?;
    let salt: [u8; SALT_LEN] = from_hex(&enc.salt)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(missing)?;
    let params = KdfParams {
        m_kib: enc.m_kib,
        t: enc.t,
        p: enc.p,
    };
    BackupKey::from_parts(params, salt, &key).map_err(err)
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Replace the live database and attachments folder with the staged ones (already
/// validated), keeping the previous state and a usable store on every failure path.
fn swap_database(
    store: &mut Store,
    live_db: &Path,
    staged_db: &Path,
    live_attachments: &Path,
    staged_attachments: &Path,
) -> CmdResult<()> {
    // Close the live connection (an in-memory placeholder takes its place).
    let placeholder = Store::open_in_memory().map_err(err)?;
    drop(std::mem::replace(store, placeholder));
    let reopen = |store: &mut Store| -> CmdResult<()> {
        *store = Store::open(live_db).map_err(err)?;
        Ok(())
    };

    for sidecar in backup::db_with_sidecars(live_db).iter().skip(1) {
        let _ = fs::remove_file(sidecar);
    }
    // Move the current attachments aside so they can be put back if anything fails.
    let previous = live_attachments.with_file_name("attachments.before-restore");
    let _ = fs::remove_dir_all(&previous);
    let had_attachments = live_attachments.exists();
    if had_attachments && let Err(e) = fs::rename(live_attachments, &previous) {
        reopen(store)?;
        return Err(format!("Couldn't replace the attachments: {e}"));
    }
    let put_back = || {
        let _ = fs::remove_dir_all(live_attachments);
        if had_attachments {
            let _ = fs::rename(&previous, live_attachments);
        }
    };
    if let Err(e) = fs::rename(staged_attachments, live_attachments) {
        put_back();
        reopen(store)?;
        return Err(format!("Couldn't replace the attachments: {e}"));
    }
    if let Err(e) = fs::rename(staged_db, live_db) {
        put_back();
        reopen(store)?;
        return Err(format!("Couldn't replace the notes database: {e}"));
    }
    reopen(store)?;
    let _ = fs::remove_dir_all(&previous);
    log::info!("restored notes and attachments from backup");
    Ok(())
}

struct RemoveOnDrop(Vec<PathBuf>);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        for p in &self.0 {
            if p.is_dir() {
                let _ = fs::remove_dir_all(p);
            } else {
                let _ = fs::remove_file(p);
            }
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
    fn never_uploads_unencrypted_while_a_backup_key_is_saved() {
        let enc = BackupEncryption {
            salt: "00".into(),
            m_kib: 65536,
            t: 3,
            p: 1,
        };
        assert!(
            check_encryption_settings(None, true).is_err(),
            "settings lost: refuse"
        );
        assert!(
            check_encryption_settings(None, false).is_ok(),
            "never encrypted"
        );
        assert!(
            check_encryption_settings(Some(&enc), true).is_ok(),
            "encrypted"
        );
    }

    #[test]
    fn hex_round_trips_and_rejects_bad_input() {
        let bytes = [0u8, 1, 0xab, 0xff, 0x10];
        assert_eq!(to_hex(&bytes), "0001abff10");
        assert_eq!(from_hex("0001abff10").unwrap(), bytes);
        assert_eq!(from_hex("0001ABFF10").unwrap(), bytes);
        assert_eq!(from_hex("").unwrap(), Vec::<u8>::new());
        for bad in ["0", "zz", "0g", "é1"] {
            assert!(from_hex(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn swap_replaces_live_database_and_keeps_store_usable() {
        let dir = std::env::temp_dir().join(format!("tuxnotes-swap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let live = dir.join("notes.db");
        let staged = dir.join("staged.db");
        let (live_att, staged_att) = (dir.join("attachments"), dir.join("staged-attachments"));
        fs::create_dir_all(live_att.join("old-file")).unwrap();
        fs::create_dir_all(staged_att.join("restored-file")).unwrap();

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
        swap_database(&mut store, &live, &staged, &live_att, &staged_att).unwrap();
        assert!(!staged.exists());
        assert!(live_att.join("restored-file").exists() && !live_att.join("old-file").exists());
        assert!(!dir.join("attachments.before-restore").exists());
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

        // A failed swap (missing staged files) leaves the live database and attachments intact.
        let mut store = Store::open(&live).unwrap();
        fs::create_dir_all(dir.join("staged2")).unwrap();
        assert!(
            swap_database(
                &mut store,
                &live,
                &dir.join("missing.db"),
                &live_att,
                &dir.join("staged2")
            )
            .is_err()
        );
        assert_eq!(store.search("restored", 10).unwrap().len(), 1);
        assert!(
            live_att.join("restored-file").exists(),
            "attachments put back"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
