//! Versioned schema migrations, tracked with SQLite's `user_version` pragma.
//!
//! Rules: never edit a migration that has shipped; append a new one instead.

use rusqlite::Connection;

use crate::error::{Error, Result};

const MIGRATIONS: &[&str] = &[
    // v1: folders, notes, full-text search.
    r#"
    CREATE TABLE folders (
        id          TEXT PRIMARY KEY NOT NULL,
        name        TEXT NOT NULL,
        is_default  INTEGER NOT NULL DEFAULT 0,
        sort_order  INTEGER NOT NULL DEFAULT 0,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL
    );
    CREATE UNIQUE INDEX folders_single_default ON folders(is_default) WHERE is_default = 1;

    -- `seq` is a stable integer key for the FTS index; `id` is the public UUIDv7.
    CREATE TABLE notes (
        seq           INTEGER PRIMARY KEY AUTOINCREMENT,
        id            TEXT NOT NULL UNIQUE,
        folder_id     TEXT NOT NULL REFERENCES folders(id) ON DELETE RESTRICT,
        title         TEXT NOT NULL DEFAULT '',
        snippet       TEXT NOT NULL DEFAULT '',
        content_json  TEXT NOT NULL,
        content_text  TEXT NOT NULL DEFAULT '',
        is_pinned     INTEGER NOT NULL DEFAULT 0,
        created_at    INTEGER NOT NULL,
        updated_at    INTEGER NOT NULL,
        deleted_at    INTEGER
    );
    CREATE INDEX notes_folder_idx  ON notes(folder_id, deleted_at);
    CREATE INDEX notes_updated_idx ON notes(updated_at);
    CREATE INDEX notes_deleted_idx ON notes(deleted_at);

    CREATE VIRTUAL TABLE notes_fts USING fts5(
        title,
        content_text,
        content = 'notes',
        content_rowid = 'seq',
        tokenize = 'unicode61 remove_diacritics 2'
    );

    CREATE TRIGGER notes_fts_ai AFTER INSERT ON notes BEGIN
        INSERT INTO notes_fts(rowid, title, content_text)
        VALUES (new.seq, new.title, new.content_text);
    END;

    CREATE TRIGGER notes_fts_ad AFTER DELETE ON notes BEGIN
        INSERT INTO notes_fts(notes_fts, rowid, title, content_text)
        VALUES ('delete', old.seq, old.title, old.content_text);
    END;

    CREATE TRIGGER notes_fts_au AFTER UPDATE OF title, content_text ON notes BEGIN
        INSERT INTO notes_fts(notes_fts, rowid, title, content_text)
        VALUES ('delete', old.seq, old.title, old.content_text);
        INSERT INTO notes_fts(rowid, title, content_text)
        VALUES (new.seq, new.title, new.content_text);
    END;
    "#,
    // v2: attachments (images and files). Files live on disk in `<attachments dir>/<id>/<filename>`;
    // notes refer to them by id only, so they survive moving between computers.
    r#"
    CREATE TABLE attachments (
        id          TEXT PRIMARY KEY NOT NULL,
        note_id     TEXT,
        filename    TEXT NOT NULL,
        mime        TEXT NOT NULL,
        size        INTEGER NOT NULL,
        sha256      TEXT NOT NULL,
        created_at  INTEGER NOT NULL
    );
    CREATE INDEX attachments_note_idx ON attachments(note_id);
    "#,
];

/// The schema version this build of TuxNotes writes.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// Bring the database up to [`SCHEMA_VERSION`]. Each migration runs in its own transaction.
pub fn migrate(conn: &mut Connection) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(Error::UnsupportedSchema {
            found: current,
            supported: SCHEMA_VERSION,
        });
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

/// Read the schema version of an open database.
pub fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}
