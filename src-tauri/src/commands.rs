//! Thin IPC layer: each command locks the store, calls one core method, and returns.
//! All business rules live in `tuxnotes-core`.

use std::sync::MutexGuard;

use serde_json::Value;
use tauri::State;
use tuxnotes_core::{Error, Folder, Note, NoteFilter, NoteSummary, Result, Store};

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
pub fn create_folder(state: State<'_, AppState>, name: String) -> Result<Folder> {
    store(&state)?.create_folder(&name)
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

#[tauri::command]
pub fn search_notes(state: State<'_, AppState>, query: String) -> Result<Vec<NoteSummary>> {
    store(&state)?.search(&query, SEARCH_LIMIT)
}
