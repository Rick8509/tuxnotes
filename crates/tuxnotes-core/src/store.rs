//! The note store: folders, notes, trash and search on top of SQLite.

use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::Value;
use uuid::Uuid;

use crate::crypto::SecretKey;
use crate::error::{Error, Result};
use crate::migrations;
use crate::models::{Folder, Note, NoteFilter, NoteSummary};
use crate::text;

/// Name of the built-in folder.
pub const DEFAULT_FOLDER_NAME: &str = "Notes";
/// Maximum folder name length in characters.
pub const MAX_FOLDER_NAME_CHARS: usize = 100;
/// How deep folders can be nested.
pub const MAX_FOLDER_DEPTH: usize = 5;
/// Maximum size of one note's serialized document (protects against runaway input).
pub const MAX_CONTENT_BYTES: usize = 20 * 1024 * 1024;
/// How long notes stay in "Recently Deleted" before they are purged.
pub const TRASH_RETENTION_DAYS: i64 = 30;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

const FOLDER_COLUMNS: &str = "f.id, f.name, f.is_default, f.sort_order, f.created_at, f.updated_at,
     (SELECT COUNT(*) FROM notes n WHERE n.folder_id = f.id AND n.deleted_at IS NULL), f.parent_id";
const SUMMARY_COLUMNS: &str =
    "id, folder_id, title, snippet, is_pinned, created_at, updated_at, deleted_at,
     (locked_blob IS NOT NULL)";
const NOTE_COLUMNS: &str = "id, folder_id, title, content_json, is_pinned, created_at, updated_at, deleted_at, locked_blob";

pub struct Store {
    pub(crate) conn: Connection,
    /// Last timestamp handed out; guarantees strictly increasing timestamps
    /// so "most recently edited" ordering is stable even within one millisecond.
    last_ts: Cell<i64>,
    /// The key for locked notes while they're unlocked (memory only; see `lock.rs`).
    pub(crate) notes_key: Option<SecretKey>,
}

impl Store {
    /// Open (or create) the database at `path` and run migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// A throwaway in-memory store, for tests.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Overwrite deleted data with zeros, so text from a note that was locked
        // doesn't linger in free pages of the file.
        conn.pragma_update(None, "secure_delete", "ON")?;
        migrations::migrate(&mut conn)?;

        let store = Store {
            conn,
            last_ts: Cell::new(0),
            notes_key: None,
        };
        store.ensure_default_folder()?;
        Ok(store)
    }

    /// Current time in Unix milliseconds, strictly increasing per store.
    pub(crate) fn now(&self) -> i64 {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let ts = wall.max(self.last_ts.get() + 1);
        self.last_ts.set(ts);
        ts
    }

    #[doc(hidden)]
    /// Raw database access for tests that check what is actually stored.
    pub fn conn_for_tests(&self) -> &Connection {
        &self.conn
    }

    fn new_id() -> String {
        Uuid::now_v7().to_string()
    }

    pub fn schema_version(&self) -> Result<i64> {
        migrations::schema_version(&self.conn)
    }

    // ------------------------------------------------------------------ folders

    fn ensure_default_folder(&self) -> Result<()> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM folders WHERE is_default = 1)",
            [],
            |row| row.get(0),
        )?;
        if !exists {
            let now = self.now();
            self.conn.execute(
                "INSERT INTO folders (id, name, is_default, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, 1, 0, ?3, ?3)",
                params![Self::new_id(), DEFAULT_FOLDER_NAME, now],
            )?;
        }
        Ok(())
    }

    pub fn default_folder_id(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT id FROM folders WHERE is_default = 1", [], |row| {
                row.get(0)
            })?)
    }

    /// All folders, each level in its own order (the default folder first at the top level).
    /// The tree is built from `parent_id`.
    pub fn list_folders(&self) -> Result<Vec<Folder>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {FOLDER_COLUMNS} FROM folders f
             ORDER BY f.is_default DESC, f.sort_order, f.name COLLATE NOCASE"
        ))?;
        let folders = stmt
            .query_map([], folder_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(folders)
    }

    pub fn get_folder(&self, id: &str) -> Result<Folder> {
        self.conn
            .query_row(
                &format!("SELECT {FOLDER_COLUMNS} FROM folders f WHERE f.id = ?1"),
                [id],
                folder_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::not_found("folder", id))
    }

    /// A new folder at the top level.
    pub fn create_folder(&self, name: &str) -> Result<Folder> {
        self.create_subfolder(name, None)
    }

    /// A new folder inside `parent_id` (or at the top level), placed last among its siblings.
    pub fn create_subfolder(&self, name: &str, parent_id: Option<&str>) -> Result<Folder> {
        if let Some(parent) = parent_id {
            self.get_folder(parent)?;
            if self.folder_depth(parent)? >= MAX_FOLDER_DEPTH {
                return Err(Error::invalid(format!(
                    "Folders can be nested at most {MAX_FOLDER_DEPTH} levels deep."
                )));
            }
        }
        let name = self.validate_folder_name(name, parent_id, None)?;
        let now = self.now();
        let id = Self::new_id();
        self.conn.execute(
            "INSERT INTO folders (id, name, is_default, parent_id, sort_order, created_at, updated_at)
             VALUES (?1, ?2, 0, ?3, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM folders), ?4, ?4)",
            params![id, name, parent_id, now],
        )?;
        self.get_folder(&id)
    }

    pub fn rename_folder(&self, id: &str, name: &str) -> Result<Folder> {
        let folder = self.get_folder(id)?;
        if folder.is_default {
            return Err(Error::invalid("The default folder can't be renamed."));
        }
        let name = self.validate_folder_name(name, folder.parent_id.as_deref(), Some(id))?;
        self.conn.execute(
            "UPDATE folders SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, name, self.now()],
        )?;
        self.get_folder(id)
    }

    /// Move a folder into `parent_id` (or to the top level) at position `index` among
    /// its new siblings. Used for both nesting and reordering by drag and drop.
    pub fn move_folder(
        &mut self,
        id: &str,
        parent_id: Option<&str>,
        index: usize,
    ) -> Result<Folder> {
        let folder = self.get_folder(id)?;
        if folder.is_default {
            return Err(Error::invalid("The default folder can't be moved."));
        }
        if let Some(parent) = parent_id {
            self.get_folder(parent)?;
            if parent == id || self.descendant_ids(id)?.iter().any(|d| d == parent) {
                return Err(Error::invalid("A folder can't be moved into itself."));
            }
            if self.folder_depth(parent)? + self.subtree_height(id)? > MAX_FOLDER_DEPTH {
                return Err(Error::invalid(format!(
                    "Folders can be nested at most {MAX_FOLDER_DEPTH} levels deep."
                )));
            }
        }
        self.validate_folder_name(&folder.name, parent_id, Some(id))?;

        // New order of the siblings, with the moved folder inserted at `index`.
        let mut siblings: Vec<String> = self
            .list_folders()?
            .into_iter()
            .filter(|f| f.parent_id.as_deref() == parent_id && f.id != id && !f.is_default)
            .map(|f| f.id)
            .collect();
        siblings.insert(index.min(siblings.len()), id.to_string());

        let now = self.now();
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE folders SET parent_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, parent_id, now],
        )?;
        for (i, sibling) in siblings.iter().enumerate() {
            tx.execute(
                "UPDATE folders SET sort_order = ?2 WHERE id = ?1",
                params![sibling, i as i64 + 1],
            )?;
        }
        tx.commit()?;
        self.get_folder(id)
    }

    /// Delete a folder and all its subfolders. Their notes move to "Recently Deleted"
    /// (and to the default folder, so they have somewhere to go if restored).
    /// Returns how many notes moved.
    pub fn delete_folder(&mut self, id: &str) -> Result<usize> {
        let folder = self.get_folder(id)?;
        if folder.is_default {
            return Err(Error::invalid("The default folder can't be deleted."));
        }
        let mut doomed = self.descendant_ids(id)?;
        doomed.insert(0, id.to_string());
        let default_id = self.default_folder_id()?;
        let now = self.now();
        let tx = self.conn.transaction()?;
        let mut moved = 0;
        for folder_id in &doomed {
            moved += tx.execute(
                "UPDATE notes
                 SET folder_id = ?2, deleted_at = COALESCE(deleted_at, ?3), is_pinned = 0
                 WHERE folder_id = ?1",
                params![folder_id, default_id, now],
            )?;
        }
        // Deepest first, so no folder is removed before its subfolders.
        for folder_id in doomed.iter().rev() {
            tx.execute("DELETE FROM folders WHERE id = ?1", [folder_id])?;
        }
        tx.commit()?;
        Ok(moved)
    }

    /// Every folder inside `id`, at any depth (parents before their children).
    pub fn descendant_ids(&self, id: &str) -> Result<Vec<String>> {
        let folders = self.list_folders()?;
        let mut out: Vec<String> = Vec::new();
        let mut queue = vec![id.to_string()];
        while let Some(current) = queue.pop() {
            for child in folders
                .iter()
                .filter(|f| f.parent_id.as_deref() == Some(current.as_str()))
            {
                if out.contains(&child.id) || child.id == id {
                    continue; // never loop, even on damaged data
                }
                out.push(child.id.clone());
                queue.push(child.id.clone());
            }
        }
        Ok(out)
    }

    /// 1 for a top-level folder, 2 for a folder inside it, and so on.
    fn folder_depth(&self, id: &str) -> Result<usize> {
        let mut depth = 1;
        let mut current = self.get_folder(id)?;
        while let Some(parent) = current.parent_id.clone() {
            depth += 1;
            if depth > 64 {
                break; // damaged data: stop rather than loop
            }
            current = self.get_folder(&parent)?;
        }
        Ok(depth)
    }

    /// Levels in a folder's subtree: 1 for a folder without subfolders.
    fn subtree_height(&self, id: &str) -> Result<usize> {
        let mut max = 1;
        for d in self.descendant_ids(id)? {
            let mut levels = 1;
            let mut current = self.get_folder(&d)?;
            while let Some(parent) = current.parent_id.clone() {
                levels += 1;
                if parent == id || levels > 64 {
                    break;
                }
                current = self.get_folder(&parent)?;
            }
            max = max.max(levels);
        }
        Ok(max)
    }

    fn validate_folder_name(
        &self,
        name: &str,
        parent_id: Option<&str>,
        existing_id: Option<&str>,
    ) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::invalid("Folder name can't be empty."));
        }
        if name.chars().count() > MAX_FOLDER_NAME_CHARS {
            return Err(Error::invalid(format!(
                "Folder name can't be longer than {MAX_FOLDER_NAME_CHARS} characters."
            )));
        }
        // Names must be unique among siblings. SQLite's NOCASE only folds ASCII,
        // so compare in Rust for full Unicode.
        let lower = name.to_lowercase();
        let taken = self.list_folders()?.into_iter().any(|f| {
            Some(f.id.as_str()) != existing_id
                && f.parent_id.as_deref() == parent_id
                && f.name.to_lowercase() == lower
        });
        if taken {
            return Err(Error::invalid(format!(
                "A folder named \"{name}\" already exists here."
            )));
        }
        Ok(name.to_string())
    }

    // -------------------------------------------------------------------- notes

    /// Create an empty note in `folder_id`, or in the default folder when `None`.
    pub fn create_note(&self, folder_id: Option<&str>) -> Result<Note> {
        let folder_id = match folder_id {
            Some(id) => self.get_folder(id)?.id,
            None => self.default_folder_id()?,
        };
        let id = Self::new_id();
        let now = self.now();
        let content = text::empty_doc();
        self.conn.execute(
            "INSERT INTO notes (id, folder_id, content_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![id, folder_id, content.to_string(), now],
        )?;
        self.get_note(&id)
    }

    /// A note with its document. For a locked note the document is decrypted when
    /// locked notes are unlocked, and `null` otherwise.
    pub fn get_note(&self, id: &str) -> Result<Note> {
        let (mut note, json, blob) = self
            .conn
            .query_row(
                &format!("SELECT {NOTE_COLUMNS} FROM notes WHERE id = ?1"),
                [id],
                |row| {
                    let blob: Option<Vec<u8>> = row.get(8)?;
                    Ok((
                        Note {
                            id: row.get(0)?,
                            folder_id: row.get(1)?,
                            title: row.get(2)?,
                            content: Value::Null,
                            is_pinned: row.get(4)?,
                            created_at: row.get(5)?,
                            updated_at: row.get(6)?,
                            deleted_at: row.get(7)?,
                            is_locked: blob.is_some(),
                        },
                        row.get::<_, String>(3)?,
                        blob,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| Error::not_found("note", id))?;
        note.content = match blob {
            None => serde_json::from_str(&json)?,
            Some(blob) => match &self.notes_key {
                Some(key) => self.decrypt_note(key, id, &blob)?,
                None => Value::Null,
            },
        };
        Ok(note)
    }

    pub fn get_note_summary(&self, id: &str) -> Result<NoteSummary> {
        self.conn
            .query_row(
                &format!("SELECT {SUMMARY_COLUMNS} FROM notes WHERE id = ?1"),
                [id],
                summary_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::not_found("note", id))
    }

    /// Save a note's document. Title, preview and search text are derived from it.
    pub fn update_note_content(&self, id: &str, content: &Value) -> Result<NoteSummary> {
        let is_doc = content.get("type").and_then(Value::as_str) == Some("doc");
        if !is_doc {
            return Err(Error::invalid("Note content must be a document."));
        }
        let json = serde_json::to_string(content)?;
        if json.len() > MAX_CONTENT_BYTES {
            return Err(Error::invalid("This note is too large to save."));
        }
        self.require_active(id)?;
        if self.is_locked(id)? {
            let key = self.require_notes_key()?;
            self.write_locked(key, id, content, true)?;
            return self.get_note_summary(id);
        }

        let extracted = text::extract(content);
        self.conn.execute(
            "UPDATE notes
             SET content_json = ?2, title = ?3, snippet = ?4, content_text = ?5, updated_at = ?6
             WHERE id = ?1",
            params![
                id,
                json,
                extracted.title,
                extracted.snippet,
                extracted.plain,
                self.now()
            ],
        )?;
        self.get_note_summary(id)
    }

    /// Copy a note (same folder, same content, not pinned). Attachments are shared by id,
    /// which is safe: they're only removed once no note refers to them.
    pub fn duplicate_note(&self, id: &str) -> Result<Note> {
        self.require_active(id)?;
        if self.is_locked(id)? {
            return Err(Error::invalid(
                "Locked notes can't be duplicated. Remove the lock first.",
            ));
        }
        let original = self.get_note(id)?;
        let copy = self.create_note(Some(&original.folder_id))?;
        self.update_note_content(&copy.id, &original.content)?;
        self.get_note(&copy.id)
    }

    /// Pinning does not change `updated_at`, matching how Apple Notes behaves.
    pub fn set_note_pinned(&self, id: &str, pinned: bool) -> Result<NoteSummary> {
        self.require_active(id)?;
        self.conn.execute(
            "UPDATE notes SET is_pinned = ?2 WHERE id = ?1",
            params![id, pinned],
        )?;
        self.get_note_summary(id)
    }

    pub fn move_note(&self, id: &str, folder_id: &str) -> Result<NoteSummary> {
        self.require_active(id)?;
        let folder = self.get_folder(folder_id)?;
        self.conn.execute(
            "UPDATE notes SET folder_id = ?2 WHERE id = ?1",
            params![id, folder.id],
        )?;
        self.get_note_summary(id)
    }

    /// Move a note to "Recently Deleted".
    pub fn trash_note(&self, id: &str) -> Result<NoteSummary> {
        self.require_active(id)?;
        self.conn.execute(
            "UPDATE notes SET deleted_at = ?2, is_pinned = 0 WHERE id = ?1",
            params![id, self.now()],
        )?;
        self.get_note_summary(id)
    }

    /// Bring a note back from "Recently Deleted" into its folder.
    pub fn restore_note(&self, id: &str) -> Result<NoteSummary> {
        let summary = self.get_note_summary(id)?;
        if summary.deleted_at.is_none() {
            return Err(Error::invalid("This note isn't in Recently Deleted."));
        }
        self.conn
            .execute("UPDATE notes SET deleted_at = NULL WHERE id = ?1", [id])?;
        self.get_note_summary(id)
    }

    /// Permanently delete one note. Only allowed from "Recently Deleted".
    pub fn delete_note_permanently(&self, id: &str) -> Result<()> {
        let summary = self.get_note_summary(id)?;
        if summary.deleted_at.is_none() {
            return Err(Error::invalid(
                "Move the note to Recently Deleted before deleting it permanently.",
            ));
        }
        self.conn.execute("DELETE FROM notes WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Delete a note that was created but never given any content (like Apple Notes
    /// does when you leave an empty new note). Returns whether it was deleted.
    pub fn discard_note_if_blank(&self, id: &str) -> Result<bool> {
        // Locked notes are never blank (their visible document is just empty).
        let row: Option<(String, Option<i64>)> = self
            .conn
            .query_row(
                "SELECT content_json, deleted_at FROM notes WHERE id = ?1 AND locked_blob IS NULL",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((json, None)) = row else {
            return Ok(false); // missing or already in the trash
        };
        let doc: Value = serde_json::from_str(&json)?;
        if !text::is_blank(&doc) {
            return Ok(false);
        }
        self.conn.execute("DELETE FROM notes WHERE id = ?1", [id])?;
        Ok(true)
    }

    /// Remove every blank note (run at startup to clean up after an unclean exit).
    pub fn discard_blank_notes(&self) -> Result<usize> {
        let ids: Vec<String> = self
            .conn
            .prepare(
                "SELECT id FROM notes
                 WHERE deleted_at IS NULL AND content_text = '' AND locked_blob IS NULL",
            )?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut removed = 0;
        for id in ids {
            if self.discard_note_if_blank(&id)? {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Permanently delete everything in "Recently Deleted". Returns the count.
    pub fn empty_trash(&self) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM notes WHERE deleted_at IS NOT NULL", [])?)
    }

    /// Purge notes that have been in "Recently Deleted" longer than the retention period.
    pub fn purge_expired_trash(&self) -> Result<usize> {
        self.purge_trash_older_than(self.now(), TRASH_RETENTION_DAYS)
    }

    /// Purge notes deleted more than `days` days before `now_ms`.
    pub fn purge_trash_older_than(&self, now_ms: i64, days: i64) -> Result<usize> {
        let cutoff = now_ms - days * DAY_MS;
        Ok(self.conn.execute(
            "DELETE FROM notes WHERE deleted_at IS NOT NULL AND deleted_at <= ?1",
            [cutoff],
        )?)
    }

    pub fn trash_count(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM notes WHERE deleted_at IS NOT NULL",
            [],
            |row| row.get(0),
        )?)
    }

    /// Notes for the note list. Pinned first, then most recently edited.
    /// Trash is ordered by deletion time instead.
    pub fn list_notes(&self, filter: &NoteFilter) -> Result<Vec<NoteSummary>> {
        let (where_clause, order, folder_param) = match filter {
            NoteFilter::All => (
                "deleted_at IS NULL",
                "is_pinned DESC, updated_at DESC, seq DESC",
                None,
            ),
            NoteFilter::Folder { folder_id } => {
                self.get_folder(folder_id)?;
                (
                    "deleted_at IS NULL AND folder_id = ?1",
                    "is_pinned DESC, updated_at DESC, seq DESC",
                    Some(folder_id.as_str()),
                )
            }
            NoteFilter::Trash => ("deleted_at IS NOT NULL", "deleted_at DESC, seq DESC", None),
        };
        let sql =
            format!("SELECT {SUMMARY_COLUMNS} FROM notes WHERE {where_clause} ORDER BY {order}");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = match folder_param {
            Some(folder_id) => stmt.query_map([folder_id], summary_from_row)?,
            None => stmt.query_map([], summary_from_row)?,
        };
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Full-text search over titles and bodies of notes not in the trash.
    /// Every word is matched as a prefix, so "gro mil" finds "Groceries: milk".
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<NoteSummary>> {
        let Some(fts_query) = build_fts_query(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.folder_id, n.title, n.snippet, n.is_pinned,
                    n.created_at, n.updated_at, n.deleted_at, (n.locked_blob IS NOT NULL)
             FROM notes_fts
             JOIN notes n ON n.seq = notes_fts.rowid
             WHERE notes_fts MATCH ?1 AND n.deleted_at IS NULL
             ORDER BY bm25(notes_fts, 10.0, 1.0), n.updated_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![fts_query, limit as i64], summary_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub(crate) fn require_active(&self, id: &str) -> Result<()> {
        if self.get_note_summary(id)?.deleted_at.is_some() {
            return Err(Error::invalid(
                "This note is in Recently Deleted. Restore it to make changes.",
            ));
        }
        Ok(())
    }

    // ------------------------------------------------------------------ backups

    /// Number of folders and notes (including notes in Recently Deleted).
    pub fn counts(&self) -> Result<(i64, i64)> {
        Ok(self.conn.query_row(
            "SELECT (SELECT COUNT(*) FROM folders), (SELECT COUNT(*) FROM notes)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    }

    /// Rows changed through this store since it was opened. Comparing two readings tells
    /// whether anything changed in between (e.g. since the last backup).
    pub fn change_count(&self) -> u64 {
        self.conn.total_changes()
    }

    /// The newest time anything was edited, created, deleted or renamed (Unix ms).
    /// Doesn't see every change (moving a note between folders keeps its time); use
    /// together with [`Store::change_count`].
    pub fn last_change_at(&self) -> Result<Option<i64>> {
        Ok(self.conn.query_row(
            "SELECT MAX(t) FROM (
                SELECT MAX(updated_at) AS t FROM notes
                UNION ALL SELECT MAX(deleted_at) FROM notes
                UNION ALL SELECT MAX(updated_at) FROM folders
                UNION ALL SELECT MAX(created_at) FROM attachments
             )",
            [],
            |row| row.get(0),
        )?)
    }

    /// Run SQLite's integrity check, including the full-text index.
    pub fn check_integrity(&self) -> Result<()> {
        let result: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if result != "ok" {
            return Err(Error::InvalidBackup(format!(
                "The database is damaged: {result}"
            )));
        }
        self.conn
            .execute(
                "INSERT INTO notes_fts(notes_fts) VALUES ('integrity-check')",
                [],
            )
            .map_err(|e| Error::InvalidBackup(format!("The search index is damaged: {e}")))?;
        Ok(())
    }

    /// Write a consistent, compacted copy of the whole database to `dest`.
    /// Safe to call while the app is running. `dest` must not exist yet.
    pub fn snapshot_to(&self, dest: impl AsRef<Path>) -> Result<()> {
        let dest = dest.as_ref();
        if dest.exists() {
            return Err(Error::invalid(format!(
                "Snapshot target already exists: {}",
                dest.display()
            )));
        }
        let dest = dest
            .to_str()
            .ok_or_else(|| Error::invalid("Snapshot path must be valid UTF-8."))?;
        self.conn.execute("VACUUM INTO ?1", [dest])?;
        Ok(())
    }
}

/// Turn user input into a safe FTS5 query: each word becomes a quoted prefix term.
fn build_fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

fn folder_from_row(row: &Row<'_>) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: row.get(0)?,
        name: row.get(1)?,
        is_default: row.get(2)?,
        sort_order: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        note_count: row.get(6)?,
        parent_id: row.get(7)?,
    })
}

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<NoteSummary> {
    Ok(NoteSummary {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        title: row.get(2)?,
        snippet: row.get(3)?,
        is_pinned: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        deleted_at: row.get(7)?,
        is_locked: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::build_fts_query;

    #[test]
    fn fts_query_quotes_and_prefixes_every_word() {
        assert_eq!(build_fts_query("  "), None);
        assert_eq!(
            build_fts_query("gro mil").as_deref(),
            Some("\"gro\"* \"mil\"*")
        );
        // FTS operators and quotes are neutralised.
        assert_eq!(
            build_fts_query("a\"b OR NEAR(").as_deref(),
            Some("\"a\"\"b\"* \"OR\"* \"NEAR(\"*")
        );
    }
}
