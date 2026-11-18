//! The note store: folders, notes, trash and search on top of SQLite.

use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::migrations;
use crate::models::{Folder, Note, NoteFilter, NoteSummary};
use crate::text;

/// Name of the built-in folder.
pub const DEFAULT_FOLDER_NAME: &str = "Notes";
/// Maximum folder name length in characters.
pub const MAX_FOLDER_NAME_CHARS: usize = 100;
/// Maximum size of one note's serialized document (protects against runaway input).
pub const MAX_CONTENT_BYTES: usize = 20 * 1024 * 1024;
/// How long notes stay in "Recently Deleted" before they are purged.
pub const TRASH_RETENTION_DAYS: i64 = 30;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

const SUMMARY_COLUMNS: &str =
    "id, folder_id, title, snippet, is_pinned, created_at, updated_at, deleted_at";
const NOTE_COLUMNS: &str =
    "id, folder_id, title, content_json, is_pinned, created_at, updated_at, deleted_at";

pub struct Store {
    conn: Connection,
    /// Last timestamp handed out; guarantees strictly increasing timestamps
    /// so "most recently edited" ordering is stable even within one millisecond.
    last_ts: Cell<i64>,
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
        migrations::migrate(&mut conn)?;

        let store = Store {
            conn,
            last_ts: Cell::new(0),
        };
        store.ensure_default_folder()?;
        Ok(store)
    }

    /// Current time in Unix milliseconds, strictly increasing per store.
    fn now(&self) -> i64 {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let ts = wall.max(self.last_ts.get() + 1);
        self.last_ts.set(ts);
        ts
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

    /// All folders: the default folder first, then by creation order.
    pub fn list_folders(&self) -> Result<Vec<Folder>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.name, f.is_default, f.sort_order, f.created_at, f.updated_at,
                    (SELECT COUNT(*) FROM notes n WHERE n.folder_id = f.id AND n.deleted_at IS NULL)
             FROM folders f
             ORDER BY f.is_default DESC, f.sort_order, f.name COLLATE NOCASE",
        )?;
        let folders = stmt
            .query_map([], folder_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(folders)
    }

    pub fn get_folder(&self, id: &str) -> Result<Folder> {
        self.conn
            .query_row(
                "SELECT f.id, f.name, f.is_default, f.sort_order, f.created_at, f.updated_at,
                        (SELECT COUNT(*) FROM notes n WHERE n.folder_id = f.id AND n.deleted_at IS NULL)
                 FROM folders f WHERE f.id = ?1",
                [id],
                folder_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::not_found("folder", id))
    }

    pub fn create_folder(&self, name: &str) -> Result<Folder> {
        let name = self.validate_folder_name(name, None)?;
        let now = self.now();
        let id = Self::new_id();
        self.conn.execute(
            "INSERT INTO folders (id, name, is_default, sort_order, created_at, updated_at)
             VALUES (?1, ?2, 0, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM folders), ?3, ?3)",
            params![id, name, now],
        )?;
        self.get_folder(&id)
    }

    pub fn rename_folder(&self, id: &str, name: &str) -> Result<Folder> {
        let folder = self.get_folder(id)?;
        if folder.is_default {
            return Err(Error::invalid("The default folder can't be renamed."));
        }
        let name = self.validate_folder_name(name, Some(id))?;
        self.conn.execute(
            "UPDATE folders SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, name, self.now()],
        )?;
        self.get_folder(id)
    }

    /// Delete a folder. Its notes move to "Recently Deleted" (and to the default
    /// folder, so they have somewhere to go if restored). Returns how many notes moved.
    pub fn delete_folder(&mut self, id: &str) -> Result<usize> {
        let folder = self.get_folder(id)?;
        if folder.is_default {
            return Err(Error::invalid("The default folder can't be deleted."));
        }
        let default_id = self.default_folder_id()?;
        let now = self.now();
        let tx = self.conn.transaction()?;
        let moved = tx.execute(
            "UPDATE notes
             SET folder_id = ?2, deleted_at = COALESCE(deleted_at, ?3), is_pinned = 0
             WHERE folder_id = ?1",
            params![id, default_id, now],
        )?;
        tx.execute("DELETE FROM folders WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(moved)
    }

    fn validate_folder_name(&self, name: &str, existing_id: Option<&str>) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::invalid("Folder name can't be empty."));
        }
        if name.chars().count() > MAX_FOLDER_NAME_CHARS {
            return Err(Error::invalid(format!(
                "Folder name can't be longer than {MAX_FOLDER_NAME_CHARS} characters."
            )));
        }
        // SQLite's NOCASE only folds ASCII, so compare in Rust for full Unicode.
        let lower = name.to_lowercase();
        let taken = self
            .list_folders()?
            .into_iter()
            .any(|f| Some(f.id.as_str()) != existing_id && f.name.to_lowercase() == lower);
        if taken {
            return Err(Error::invalid(format!(
                "A folder named \"{name}\" already exists."
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

    pub fn get_note(&self, id: &str) -> Result<Note> {
        self.conn
            .query_row(
                &format!("SELECT {NOTE_COLUMNS} FROM notes WHERE id = ?1"),
                [id],
                |row| {
                    let json: String = row.get(3)?;
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
                        },
                        json,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| Error::not_found("note", id))
            .and_then(|(mut note, json)| {
                note.content = serde_json::from_str(&json)?;
                Ok(note)
            })
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
                    n.created_at, n.updated_at, n.deleted_at
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

    fn require_active(&self, id: &str) -> Result<()> {
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
