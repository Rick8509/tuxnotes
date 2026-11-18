//! Backup archives: a zip holding `manifest.json` plus a consistent copy of `notes.db`.
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

use crate::error::{Error, Result};
use crate::migrations::SCHEMA_VERSION;
use crate::store::Store;

/// Version of the archive layout. Bump only for incompatible changes.
pub const BACKUP_FORMAT_VERSION: u32 = 1;
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

/// Write a backup of `store` to `dest` (which must not exist). The archive is
/// written to a temporary file first and renamed into place, so `dest` is
/// either complete or absent.
pub fn create_backup(store: &Store, dest: &Path, app_version: &str) -> Result<Manifest> {
    if dest.exists() {
        return Err(Error::invalid(format!(
            "Backup target already exists: {}",
            dest.display()
        )));
    }
    let snapshot = sibling(dest, "snapshot.db");
    let partial = sibling(dest, "partial");
    let _cleanup = Cleanup(vec![snapshot.clone(), partial.clone()]);
    remove_if_exists(&snapshot)?;
    remove_if_exists(&partial)?;

    store.snapshot_to(&snapshot)?;
    let (folder_count, note_count) = store.counts()?;
    let (database_sha256, database_size) = sha256_file(&snapshot)?;

    let manifest = Manifest {
        app: BACKUP_APP_ID.to_string(),
        format_version: BACKUP_FORMAT_VERSION,
        app_version: app_version.to_string(),
        schema_version: store.schema_version()?,
        created_at: now_ms(),
        folder_count,
        note_count,
        database_size,
        database_sha256,
    };

    {
        let mut zip = ZipWriter::new(File::create(&partial)?);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .large_file(database_size >= u32::MAX as u64);
        zip.start_file(MANIFEST_FILE, options)?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        zip.start_file(DATABASE_FILE, options)?;
        io::copy(&mut File::open(&snapshot)?, &mut zip)?;
        zip.finish()?.sync_all()?;
    }
    fs::rename(&partial, dest)?;
    Ok(manifest)
}

/// Read and validate just the manifest of a backup archive.
pub fn read_manifest(archive: &Path) -> Result<Manifest> {
    let mut zip = open_archive(archive)?;
    read_manifest_from(&mut zip)
}

/// Fully validate a backup and unpack its database to `dest_db` (must not exist).
///
/// Checks: it's a TuxNotes archive of a supported version, the database matches
/// its checksum, it opens, migrates to the current schema, and passes SQLite's
/// integrity check. On any failure `dest_db` is removed.
pub fn extract_backup(archive: &Path, dest_db: &Path) -> Result<Manifest> {
    if dest_db.exists() {
        return Err(Error::invalid(format!(
            "Restore target already exists: {}",
            dest_db.display()
        )));
    }
    let mut zip = open_archive(archive)?;
    let manifest = read_manifest_from(&mut zip)?;

    let result = (|| {
        {
            let entry = zip.by_name(DATABASE_FILE).map_err(|_| {
                Error::InvalidBackup("The backup doesn't contain a notes database.".into())
            })?;
            let mut out = File::create(dest_db)?;
            let mut hasher = Sha256::new();
            let mut limited = entry.take(MAX_DATABASE_BYTES + 1);
            let mut buf = vec![0u8; 64 * 1024];
            let mut total: u64 = 0;
            loop {
                let n = limited.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                total += n as u64;
                if total > MAX_DATABASE_BYTES {
                    return Err(Error::InvalidBackup("The backup is too large.".into()));
                }
                hasher.update(&buf[..n]);
                out.write_all(&buf[..n])?;
            }
            out.sync_all()?;
            if total != manifest.database_size
                || to_hex(&hasher.finalize()) != manifest.database_sha256
            {
                return Err(Error::InvalidBackup(
                    "The backup is damaged (checksum mismatch).".into(),
                ));
            }
        }
        // Opening runs migrations, so older backups are upgraded here.
        let restored = Store::open(dest_db)?;
        restored.check_integrity()?;
        Ok(())
    })();

    if let Err(e) = result {
        for path in db_with_sidecars(dest_db) {
            let _ = fs::remove_file(path);
        }
        return Err(e);
    }
    Ok(manifest)
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
