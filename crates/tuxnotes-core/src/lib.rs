//! # tuxnotes-core
//!
//! The GUI-independent heart of TuxNotes: a SQLite-backed note store with
//! folders, a "Recently Deleted" trash, full-text search, and database snapshots
//! for backups.
//!
//! This crate deliberately has no Tauri/GTK dependencies so it can be built and
//! tested anywhere.

pub mod attachments;
pub mod backup;
pub mod crypto;
pub mod error;
pub mod lock;
pub mod migrations;
pub mod models;
pub mod store;
pub mod text;

pub use attachments::Attachment;
pub use backup::Manifest;
pub use error::{Error, Result};
pub use lock::LockStatus;
pub use models::{Folder, Note, NoteFilter, NoteSummary};
pub use store::Store;
