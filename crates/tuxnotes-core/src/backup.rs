//! Backup archives: a zip holding `manifest.json`, a consistent copy of `notes.db`
//! and (format 2+) the note attachments under `attachments/<id>/<filename>`.
//!
//! Creating a backup never blocks the running app for long (it uses `VACUUM INTO`),
//! and restoring is two-phase: [`extract_backup`] fully validates the archive into a
//! *new* file, and only then does the caller swap it in. A bad backup can therefore
//! never damage the live database.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::attachments::{MAX_ATTACHMENT_BYTES, is_safe_filename, is_valid_id};
use crate::error::{Error, Result};
use crate::migrations::SCHEMA_VERSION;
use crate::store::Store;

/// Version of the archive layout. Bump only for incompatible changes.
/// 1: manifest + notes.db. 2: adds attachments.
pub const BACKUP_FORMAT_VERSION: u32 = 2;
/// Folder inside the archive that holds attachments.
pub const ATTACHMENTS_PREFIX: &str = "attachments/";
/// Identifies TuxNotes archives.
pub const BACKUP_APP_ID: &str = "TuxNotes";
pub const MANIFEST_FILE: &str = "manifest.json";
pub const DATABASE_FILE: &str = "notes.db";
/// Refuse to unpack databases larger than this (protects against zip bombs).
pub const MAX_DATABASE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub app: String,
    pub format_version: u32,
    pub app_version: String,
    pub schema_version: i64,
    /// Unix milliseconds.
    pub created_at: i64,
    pub folder_count: i64,
    pub note_count: i64,
    pub database_size: u64,
    /// Lower-case hex SHA-256 of `notes.db`.
    pub database_sha256: String,
    /// Attachment files in the archive (empty for format 1).
    #[serde(default)]
    pub attachments: Vec<AttachmentEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentEntry {
    /// `<id>/<filename>`, relative to the attachments folder.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// File name for a new backup, e.g. `tuxnotes-backup-20261119-140533.zip` (UTC).
pub fn backup_file_name(created_at_ms: i64) -> String {
    let secs = created_at_ms.div_euclid(1000);
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "tuxnotes-backup-{y:04}{mo:02}{d:02}-{:02}{:02}{:02}.zip",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Write a backup of `store` (and the files in `attachments_dir`, if given) to `dest`,
/// which must not exist. The archive is written to a temporary file first and renamed
/// into place, so `dest` is either complete or absent.
///
/// Apps should call [`snapshot_for_backup`] while holding their store and
/// [`write_backup`] after letting go of it; this does both in one go.
pub fn create_backup(
    store: &Store,
    attachments_dir: Option<&Path>,
    dest: &Path,
    app_version: &str,
) -> Result<Manifest> {
    let snapshot = snapshot_for_backup(store, dest)?;
    write_backup(snapshot, attachments_dir, dest, app_version)
}

/// The part of a backup that needs the store: a consistent copy of the database plus
/// what the manifest says about it. Taking it is quick; writing the archive (which can
/// take minutes with many pictures) doesn't need the store, so notes can still be
/// saved meanwhile. The copy is deleted when this is dropped.
pub struct BackupSnapshot {
    db: PathBuf,
    folder_count: i64,
    note_count: i64,
    schema_version: i64,
    _cleanup: Cleanup,
}

/// Take the database copy for a backup that will be written to `dest`.
pub fn snapshot_for_backup(store: &Store, dest: &Path) -> Result<BackupSnapshot> {
    if dest.exists() {
        return Err(Error::invalid(format!(
            "Backup target already exists: {}",
            dest.display()
        )));
    }
    let db = sibling(dest, "snapshot.db");
    let cleanup = Cleanup(vec![db.clone()]);
    remove_if_exists(&db)?;
    store.snapshot_to(&db)?;
    let (folder_count, note_count) = store.counts()?;
    Ok(BackupSnapshot {
        db,
        folder_count,
        note_count,
        schema_version: store.schema_version()?,
        _cleanup: cleanup,
    })
}

/// Write the backup archive from a snapshot (no store needed).
pub fn write_backup(
    snapshot: BackupSnapshot,
    attachments_dir: Option<&Path>,
    dest: &Path,
    app_version: &str,
) -> Result<Manifest> {
    if dest.exists() {
        return Err(Error::invalid(format!(
            "Backup target already exists: {}",
            dest.display()
        )));
    }
    let partial = sibling(dest, "partial");
    let _cleanup = Cleanup(vec![partial.clone()]);
    remove_if_exists(&partial)?;

    let (database_sha256, database_size) = sha256_file(&snapshot.db)?;
    // Attachments listed in the snapshot, so the files match the database exactly.
    let attachment_rows = match attachments_dir {
        Some(_) => attachments_in(&snapshot.db)?,
        None => Vec::new(),
    };

    let mut attachments = Vec::new();
    let mut zip = ZipWriter::new(File::create(&partial)?);
    let options = |size: u64, method: CompressionMethod| {
        SimpleFileOptions::default()
            .compression_method(method)
            .large_file(size >= u32::MAX as u64)
    };
    zip.start_file(
        DATABASE_FILE,
        options(database_size, CompressionMethod::Deflated),
    )?;
    io::copy(&mut File::open(&snapshot.db)?, &mut zip)?;

    if let Some(dir) = attachments_dir {
        for (id, filename) in attachment_rows {
            // A missing file is skipped rather than failing the whole backup.
            let Ok(mut file) = File::open(dir.join(&id).join(&filename)) else {
                continue;
            };
            let path = format!("{id}/{filename}");
            let method = if already_compressed(&filename) {
                CompressionMethod::Stored
            } else {
                CompressionMethod::Deflated
            };
            let size = file.metadata().map(|m| m.len()).unwrap_or(0);
            zip.start_file(format!("{ATTACHMENTS_PREFIX}{path}"), options(size, method))?;
            let mut hasher = Sha256::new();
            let size = io::copy(&mut file, &mut Tee(&mut zip, &mut hasher))?;
            attachments.push(AttachmentEntry {
                path,
                size,
                sha256: to_hex(&hasher.finalize()),
            });
        }
    }

    let manifest = Manifest {
        app: BACKUP_APP_ID.to_string(),
        format_version: BACKUP_FORMAT_VERSION,
        app_version: app_version.to_string(),
        schema_version: snapshot.schema_version,
        created_at: now_ms(),
        folder_count: snapshot.folder_count,
        note_count: snapshot.note_count,
        database_size,
        database_sha256,
        attachments,
    };
    zip.start_file(MANIFEST_FILE, options(0, CompressionMethod::Deflated))?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    zip.finish()?.sync_all()?;
    fs::rename(&partial, dest)?;
    Ok(manifest)
}

/// Pictures, videos and archives barely shrink; storing them as they are is much faster.
fn already_compressed(filename: &str) -> bool {
    let ext = filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase());
    matches!(
        ext.as_deref(),
        Some(
            "jpg"
                | "jpeg"
                | "png"
                | "gif"
                | "webp"
                | "avif"
                | "heic"
                | "heif"
                | "mp4"
                | "mov"
                | "mkv"
                | "webm"
                | "mp3"
                | "m4a"
                | "ogg"
                | "opus"
                | "zip"
                | "gz"
                | "xz"
                | "bz2"
                | "7z"
                | "zst"
                | "docx"
                | "xlsx"
                | "pptx"
                | "odt"
                | "ods"
                | "odp"
                | "epub"
        )
    )
}

/// `(id, filename)` of every attachment recorded in a database file (read-only).
fn attachments_in(db: &Path) -> Result<Vec<(String, String)>> {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let has_table: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'attachments')",
        [],
        |row| row.get(0),
    )?;
    if !has_table {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT id, filename FROM attachments ORDER BY created_at")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter(|(id, name)| is_valid_id(id) && is_safe_filename(name))
        .collect())
}

/// Read and validate just the manifest of a backup archive.
pub fn read_manifest(archive: &Path) -> Result<Manifest> {
    let mut zip = open_archive(archive)?;
    read_manifest_from(&mut zip)
}

/// Fully validate a backup and unpack it: the database to `dest_db` and, if
/// `dest_attachments` is given, the attachments into that folder. Neither may exist yet.
///
/// Checks: it's a TuxNotes archive of a supported version, every file matches its
/// checksum, attachment paths are safe, and the database opens, migrates to the current
/// schema and passes SQLite's integrity check. On any failure nothing is left behind.
pub fn extract_backup(
    archive: &Path,
    dest_db: &Path,
    dest_attachments: Option<&Path>,
) -> Result<Manifest> {
    if dest_db.exists() || dest_attachments.is_some_and(Path::exists) {
        return Err(Error::invalid(format!(
            "Restore target already exists: {}",
            dest_db.display()
        )));
    }
    let mut zip = open_archive(archive)?;
    let manifest = read_manifest_from(&mut zip)?;

    let result = (|| {
        extract_verified(
            &mut zip,
            DATABASE_FILE,
            dest_db,
            MAX_DATABASE_BYTES,
            manifest.database_size,
            &manifest.database_sha256,
        )?;
        // Opening runs migrations, so older backups are upgraded here.
        let restored = Store::open(dest_db)?;
        restored.check_integrity()?;
        restored.check_attachment_records()?;
        drop(restored);

        if let Some(dir) = dest_attachments {
            fs::create_dir_all(dir)?;
            for entry in &manifest.attachments {
                let (id, name) = entry.path.split_once('/').unwrap_or(("", ""));
                if !is_valid_id(id) || !is_safe_filename(name) {
                    return Err(Error::InvalidBackup(
                        "The backup contains an unsafe attachment path.".into(),
                    ));
                }
                fs::create_dir_all(dir.join(id))?;
                extract_verified(
                    &mut zip,
                    &format!("{ATTACHMENTS_PREFIX}{}", entry.path),
                    &dir.join(id).join(name),
                    MAX_ATTACHMENT_BYTES,
                    entry.size,
                    &entry.sha256,
                )?;
            }
        }
        Ok(())
    })();

    if let Err(e) = result {
        for path in db_with_sidecars(dest_db) {
            let _ = fs::remove_file(path);
        }
        if let Some(dir) = dest_attachments {
            let _ = fs::remove_dir_all(dir);
        }
        return Err(e);
    }
    Ok(manifest)
}

/// Copy one archive entry to `dest`, enforcing a size limit and the expected checksum.
fn extract_verified(
    zip: &mut ZipArchive<File>,
    name: &str,
    dest: &Path,
    max: u64,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<()> {
    let entry = zip
        .by_name(name)
        .map_err(|_| Error::InvalidBackup(format!("The backup is incomplete (missing {name}).")))?;
    let mut out = File::create(dest)?;
    let mut hasher = Sha256::new();
    let mut limited = entry.take(max + 1);
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = limited.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > max {
            return Err(Error::InvalidBackup("The backup is too large.".into()));
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
    }
    out.sync_all()?;
    if total != expected_size || to_hex(&hasher.finalize()) != expected_sha256 {
        return Err(Error::InvalidBackup(
            "The backup is damaged (checksum mismatch).".into(),
        ));
    }
    Ok(())
}

/// Keep only the `keep` newest files in `dir` whose names start with `prefix`
/// and end with `.zip`. Names must sort chronologically (ours do). Returns how many were removed.
pub fn prune_local_backups(dir: &Path, prefix: &str, keep: usize) -> Result<usize> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut names: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && n.ends_with(".zip"))
        })
        .collect();
    names.sort();
    let excess = names.len().saturating_sub(keep);
    for path in &names[..excess] {
        fs::remove_file(path)?;
    }
    Ok(excess)
}

/// The database file plus its WAL/SHM sidecar files.
pub fn db_with_sidecars(db: &Path) -> [PathBuf; 3] {
    let with = |suffix: &str| {
        let mut s = db.as_os_str().to_owned();
        s.push(suffix);
        PathBuf::from(s)
    };
    [db.to_path_buf(), with("-wal"), with("-shm")]
}

// ------------------------------------------------------------------ helpers

fn open_archive(path: &Path) -> Result<ZipArchive<File>> {
    ZipArchive::new(File::open(path)?)
        .map_err(|_| Error::InvalidBackup("This file isn't a TuxNotes backup.".into()))
}

fn read_manifest_from(zip: &mut ZipArchive<File>) -> Result<Manifest> {
    let not_ours = || Error::InvalidBackup("This file isn't a TuxNotes backup.".into());
    let mut raw = String::new();
    zip.by_name(MANIFEST_FILE)
        .map_err(|_| not_ours())?
        .take(1024 * 1024)
        .read_to_string(&mut raw)?;
    let manifest: Manifest = serde_json::from_str(&raw).map_err(|_| not_ours())?;
    if manifest.app != BACKUP_APP_ID {
        return Err(not_ours());
    }
    if manifest.format_version > BACKUP_FORMAT_VERSION || manifest.schema_version > SCHEMA_VERSION {
        return Err(Error::InvalidBackup(
            "This backup was made by a newer version of TuxNotes. Please update the app to restore it."
                .into(),
        ));
    }
    Ok(manifest)
}

fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let size = io::copy(&mut file, &mut HashWriter(&mut hasher))?;
    Ok((to_hex(&hasher.finalize()), size))
}

/// Writes to an inner writer while hashing what passes through.
struct Tee<'a, W: Write>(&'a mut W, &'a mut Sha256);

impl<W: Write> Write for Tee<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.0.write(buf)?;
        self.1.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

struct HashWriter<'a>(&'a mut Sha256);

impl Write for HashWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(suffix);
    PathBuf::from(s)
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Deletes temporary files when dropped.
struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_use_utc_timestamps() {
        assert_eq!(backup_file_name(0), "tuxnotes-backup-19700101-000000.zip");
        // 2026-11-19 14:05:33 UTC
        assert_eq!(
            backup_file_name(1_795_097_133_000),
            "tuxnotes-backup-20261119-140533.zip"
        );
        // Leap day.
        assert_eq!(
            backup_file_name(1_709_164_800_000),
            "tuxnotes-backup-20240229-000000.zip"
        );
    }
}
