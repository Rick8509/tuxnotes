use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A folder in the sidebar, with the number of (non-deleted) notes it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub name: String,
    /// The built-in "Notes" folder. It cannot be renamed, moved or deleted.
    pub is_default: bool,
    /// The folder this one is inside, or `None` at the top level.
    pub parent_id: Option<String>,
    pub sort_order: i64,
    pub note_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Lightweight note data for the note list (no full content).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub id: String,
    pub folder_id: String,
    pub title: String,
    pub snippet: String,
    pub is_pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// Set when the note is in "Recently Deleted".
    pub deleted_at: Option<i64>,
}

/// A full note including its rich-text document (TipTap/ProseMirror JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub folder_id: String,
    pub title: String,
    pub content: Value,
    pub is_pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

/// Which notes to show in the note list.
///
/// JSON form: `{"kind":"all"}`, `{"kind":"folder","folderId":"…"}`, `{"kind":"trash"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NoteFilter {
    All,
    Folder { folder_id: String },
    Trash,
}
