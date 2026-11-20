//! Thin IPC layer: each command locks the store, calls one core method, and returns.
//! All business rules live in `tuxnotes-core`.

use std::sync::MutexGuard;

use serde_json::Value;
use tauri::State;
use tuxnotes_core::{Error, Folder, LockStatus, Note, NoteFilter, NoteSummary, Result, Store};
use zeroize::Zeroizing;

use crate::AppState;

const SEARCH_LIMIT: usize = 200;

fn store<'a>(state: &'a State<'_, AppState>) -> Result<MutexGuard<'a, Store>> {
    state
        .store
        .lock()
        .map_err(|_| Error::InvalidInput("Internal error: the note store is unavailable.".into()))
}

// ---------------------------------------------------------------- folders

#[tauri::command]
pub fn list_folders(state: State<'_, AppState>) -> Result<Vec<Folder>> {
    store(&state)?.list_folders()
}

#[tauri::command]
pub fn create_folder(
    state: State<'_, AppState>,
    name: String,
    parent_id: Option<String>,
) -> Result<Folder> {
    store(&state)?.create_subfolder(&name, parent_id.as_deref())
}

/// Nest or reorder: put folder `id` into `parent_id` (None = top level) at position `index`.
#[tauri::command]
pub fn move_folder(
    state: State<'_, AppState>,
    id: String,
    parent_id: Option<String>,
    index: usize,
) -> Result<Folder> {
    store(&state)?.move_folder(&id, parent_id.as_deref(), index)
}

#[tauri::command]
pub fn rename_folder(state: State<'_, AppState>, id: String, name: String) -> Result<Folder> {
    store(&state)?.rename_folder(&id, &name)
}

#[tauri::command]
pub fn delete_folder(state: State<'_, AppState>, id: String) -> Result<usize> {
    store(&state)?.delete_folder(&id)
}

// ------------------------------------------------------------------ notes

#[tauri::command]
pub fn list_notes(state: State<'_, AppState>, filter: NoteFilter) -> Result<Vec<NoteSummary>> {
    store(&state)?.list_notes(&filter)
}

#[tauri::command]
pub fn get_note(state: State<'_, AppState>, id: String) -> Result<Note> {
    store(&state)?.get_note(&id)
}

#[tauri::command]
pub fn create_note(state: State<'_, AppState>, folder_id: Option<String>) -> Result<Note> {
    store(&state)?.create_note(folder_id.as_deref())
}

#[tauri::command]
pub fn update_note_content(
    state: State<'_, AppState>,
    id: String,
    content: Value,
) -> Result<NoteSummary> {
    store(&state)?.update_note_content(&id, &content)
}

#[tauri::command]
pub fn duplicate_note(state: State<'_, AppState>, id: String) -> Result<Note> {
    store(&state)?.duplicate_note(&id)
}

#[tauri::command]
pub fn set_note_pinned(
    state: State<'_, AppState>,
    id: String,
    pinned: bool,
) -> Result<NoteSummary> {
    store(&state)?.set_note_pinned(&id, pinned)
}

#[tauri::command]
pub fn move_note(state: State<'_, AppState>, id: String, folder_id: String) -> Result<NoteSummary> {
    store(&state)?.move_note(&id, &folder_id)
}

#[tauri::command]
pub fn trash_note(state: State<'_, AppState>, id: String) -> Result<NoteSummary> {
    store(&state)?.trash_note(&id)
}

#[tauri::command]
pub fn restore_note(state: State<'_, AppState>, id: String) -> Result<NoteSummary> {
    store(&state)?.restore_note(&id)
}

#[tauri::command]
pub fn delete_note_permanently(state: State<'_, AppState>, id: String) -> Result<()> {
    store(&state)?.delete_note_permanently(&id)
}

#[tauri::command]
pub fn empty_trash(state: State<'_, AppState>) -> Result<usize> {
    store(&state)?.empty_trash()
}

#[tauri::command]
pub fn trash_count(state: State<'_, AppState>) -> Result<i64> {
    store(&state)?.trash_count()
}

/// Changes when another program wrote to the notes (the window then reloads them).
#[tauri::command]
pub fn data_version(state: State<'_, AppState>) -> Result<i64> {
    store(&state)?.data_version()
}

#[tauri::command]
pub fn discard_note_if_blank(state: State<'_, AppState>, id: String) -> Result<bool> {
    store(&state)?.discard_note_if_blank(&id)
}

#[tauri::command]
pub fn search_notes(state: State<'_, AppState>, query: String) -> Result<Vec<NoteSummary>> {
    store(&state)?.search(&query, SEARCH_LIMIT)
}

// ----------------------------------------------------------------- backup

use crate::backup_service::{BackupInfo, BackupStatus, CmdResult, RestoreResult};
use crate::settings::AutoBackup;

#[tauri::command]
pub fn backup_status(state: State<'_, AppState>) -> BackupStatus {
    state.backup.status()
}

#[tauri::command]
pub fn set_backup_preferences(
    state: State<'_, AppState>,
    auto_backup: AutoBackup,
    keep_backups: u32,
    backup_on_close: bool,
) -> BackupStatus {
    state
        .backup
        .set_preferences(auto_backup, keep_backups, backup_on_close)
}

/// Encrypt backups with a password (or change it); `None` turns encryption off.
#[tauri::command]
pub async fn set_backup_password(
    state: State<'_, AppState>,
    password: Option<String>,
) -> CmdResult<BackupStatus> {
    state.backup.set_backup_password(password).await
}

/// Quick check when the window closes: is there anything to back up first?
#[tauri::command]
pub fn backup_on_close_needed(state: State<'_, AppState>) -> bool {
    match state.store.lock() {
        Ok(store) => state.backup.backup_needed_on_close(&store),
        Err(_) => false,
    }
}

#[tauri::command]
pub async fn backup_before_exit(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<bool> {
    state.backup.backup_before_exit(&app).await
}

#[tauri::command]
pub async fn google_sign_in(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<BackupStatus> {
    state.backup.sign_in(&app).await
}

#[tauri::command]
pub fn google_cancel_sign_in(state: State<'_, AppState>) {
    state.backup.cancel_sign_in()
}

#[tauri::command]
pub async fn google_sign_out(state: State<'_, AppState>) -> CmdResult<BackupStatus> {
    state.backup.sign_out().await
}

#[tauri::command]
pub async fn backup_now(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<BackupInfo> {
    state.backup.backup_now(&app).await
}

#[tauri::command]
pub async fn list_backups(state: State<'_, AppState>) -> CmdResult<Vec<BackupInfo>> {
    state.backup.list_backups().await
}

#[tauri::command]
pub async fn restore_backup(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    password: Option<String>,
) -> CmdResult<RestoreResult> {
    state.backup.restore(&app, &id, password).await
}

// ------------------------------------------------------------------ about

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    data_dir: String,
    database: String,
    config_dir: String,
}

/// Version and storage locations, for Settings → About.
#[tauri::command]
pub fn app_info(app: tauri::AppHandle) -> std::result::Result<AppInfo, String> {
    use tauri::Manager;
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        database: data_dir.join("notes.db").display().to_string(),
        data_dir: data_dir.display().to_string(),
        config_dir: config_dir.display().to_string(),
    })
}

// ------------------------------------------------------------ locked notes
// Password checks take a noticeable fraction of a second (on purpose), so these
// run off the main thread: `async` keeps the window responsive meanwhile.

#[tauri::command]
pub fn lock_status(state: State<'_, AppState>) -> Result<LockStatus> {
    store(&state)?.lock_status()
}

#[tauri::command(async)]
pub fn set_lock_password(
    state: State<'_, AppState>,
    password: String,
    hint: String,
) -> Result<LockStatus> {
    let password = Zeroizing::new(password);
    let mut store = store(&state)?;
    store.set_lock_password(&password, &hint)?;
    store.lock_status()
}

#[tauri::command(async)]
pub fn change_lock_password(
    state: State<'_, AppState>,
    old_password: String,
    new_password: String,
    hint: String,
) -> Result<LockStatus> {
    let (old_password, new_password) = (Zeroizing::new(old_password), Zeroizing::new(new_password));
    let mut store = store(&state)?;
    store.change_lock_password(&old_password, &new_password, &hint)?;
    store.lock_status()
}

#[tauri::command(async)]
pub fn unlock_notes(state: State<'_, AppState>, password: String) -> Result<LockStatus> {
    let password = Zeroizing::new(password);
    let mut store = store(&state)?;
    store.unlock_notes(&password)?;
    store.lock_status()
}

#[tauri::command]
pub fn lock_notes(state: State<'_, AppState>) -> Result<LockStatus> {
    let mut store = store(&state)?;
    store.lock_notes();
    store.lock_status()
}

#[tauri::command]
pub fn lock_note(state: State<'_, AppState>, id: String) -> Result<NoteSummary> {
    store(&state)?.lock_note(&id)
}

#[tauri::command]
pub fn remove_note_lock(state: State<'_, AppState>, id: String) -> Result<NoteSummary> {
    store(&state)?.remove_note_lock(&id)
}
