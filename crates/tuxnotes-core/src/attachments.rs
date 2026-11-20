//! Attachments: images and files added to notes.
//!
//! Each attachment is stored on disk as `<attachments dir>/<id>/<filename>` and has a row
//! in the `attachments` table. Notes refer to attachments only by `attachmentId` (in the
//! note's JSON), never by path, so notes stay valid after a backup is restored elsewhere.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::store::Store;

/// Largest single file that can be attached.
pub const MAX_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;
/// Unused attachments are only removed after this long, so "undo" still works.
pub const UNUSED_GRACE_MS: i64 = 24 * 60 * 60 * 1000;
const MAX_FILENAME_CHARS: usize = 120;
/// File names are also limited in bytes: Linux allows 255, encrypted home folders
/// (eCryptfs) only 143, and the name gets a ".partial" ending while it's written.
const MAX_FILENAME_BYTES: usize = 130;
const PARTIAL_SUFFIX: &str = ".partial";

/// Image types shown inline in notes. Everything else is shown as a file.
pub const IMAGE_MIMES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/avif",
    "image/bmp",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    /// The note it was first added to (informational).
    pub note_id: Option<String>,
    pub filename: String,
    pub mime: String,
    pub size: u64,
    pub sha256: String,
    pub created_at: i64,
}

impl Attachment {
    pub fn is_image(&self) -> bool {
        IMAGE_MIMES.contains(&self.mime.as_str())
    }

    /// Path relative to the attachments directory: `<id>/<filename>`.
    pub fn relative_path(&self) -> String {
        format!("{}/{}", self.id, self.filename)
    }
}

/// Attachment ids are UUIDs; anything else is rejected before touching the disk.
pub fn is_valid_id(id: &str) -> bool {
    id.len() == 36 && Uuid::parse_str(id).is_ok()
}

/// Whether a stored file name is safe to use as one path component inside an
/// attachment's folder. Records from a restored backup are checked with this, so a
/// tampered backup can't point TuxNotes at files elsewhere on the computer.
pub fn is_safe_filename(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| c == '/' || c == '\\' || c.is_control())
}

/// Make a file name safe to use as a single path component, keeping it readable.
pub fn sanitize_filename(name: &str) -> String {
    // Only the last component of anything path-like.
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            c if c.is_control() => '_',
            ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let mut out = if trimmed.is_empty() {
        "attachment".to_string()
    } else {
        trimmed.to_string()
    };
    if out.chars().count() > MAX_FILENAME_CHARS || out.len() > MAX_FILENAME_BYTES {
        // Keep the extension when shortening.
        let (stem, ext) = match out.rfind('.') {
            Some(i) if out.len() - i <= 10 => (out[..i].to_string(), out[i..].to_string()),
            _ => (out.clone(), String::new()),
        };
        let keep_chars = MAX_FILENAME_CHARS - ext.chars().count();
        let keep_bytes = MAX_FILENAME_BYTES - ext.len();
        // Cut between characters, within both limits.
        let mut cut = String::new();
        for (n, c) in stem.chars().enumerate() {
            if n >= keep_chars || cut.len() + c.len_utf8() > keep_bytes {
                break;
            }
            cut.push(c);
        }
        // Trim the cut end too, so cleaning a cleaned name changes nothing.
        let cut = cut.trim_end().trim_end_matches('.').trim_end();
        out = if cut.is_empty() { "attachment" } else { cut }.to_string() + &ext;
    }
    if out.ends_with(PARTIAL_SUFFIX) {
        out.push('_');
    }
    out
}

/// Recognise the common image formats from their first bytes.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    let starts = |sig: &[u8]| bytes.starts_with(sig);
    if starts(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if starts(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(&bytes[8..12], b"avif" | b"avis")
    {
        Some("image/avif")
    } else if starts(b"BM") && bytes.len() > 26 {
        Some("image/bmp")
    } else {
        None
    }
}

/// A reasonable MIME type from a file name, for files that aren't images.
pub fn mime_from_name(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "svg" => "image/svg+xml",
        "heic" => "image/heic",
        _ => "application/octet-stream",
    }
}

/// Every `attachmentId` referenced anywhere in a note document.
pub fn referenced_ids(doc: &Value, out: &mut HashSet<String>) {
    if let Some(id) = doc
        .get("attrs")
        .and_then(|a| a.get("attachmentId"))
        .and_then(Value::as_str)
    {
        out.insert(id.to_string());
    }
    if let Some(children) = doc.get("content").and_then(Value::as_array) {
        for child in children {
            referenced_ids(child, out);
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn attachment_from_row(row: &Row<'_>) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: row.get(0)?,
        note_id: row.get(1)?,
        filename: row.get(2)?,
        mime: row.get(3)?,
        size: row.get::<_, i64>(4)? as u64,
        sha256: row.get(5)?,
        created_at: row.get(6)?,
    })
}

const COLUMNS: &str = "id, note_id, filename, mime, size, sha256, created_at";

impl Store {
    /// Save a file into `dir` and record it. Images are recognised by their bytes,
    /// not by the claimed type.
    pub fn add_attachment(
        &self,
        dir: &Path,
        note_id: Option<&str>,
        filename: &str,
        claimed_mime: &str,
        bytes: &[u8],
    ) -> Result<Attachment> {
        let filename = sanitize_filename(filename);
        if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
            return Err(Error::invalid(format!(
                "“{filename}” is too large to attach (the limit is {} MB).",
                MAX_ATTACHMENT_BYTES / 1024 / 1024
            )));
        }
        if let Some(id) = note_id {
            self.get_note_summary(id)?;
        }
        let mime = match sniff_image(bytes) {
            Some(image) => image.to_string(),
            None => {
                let claimed = claimed_mime.trim().to_ascii_lowercase();
                // A claimed image type we couldn't verify is treated as a plain file.
                if claimed.is_empty() || claimed.starts_with("image/") || !claimed.contains('/') {
                    mime_from_name(&filename).to_string()
                } else {
                    claimed
                }
            }
        };

        let id = Uuid::now_v7().to_string();
        let folder = dir.join(&id);
        fs::create_dir_all(&folder)?;
        let result = (|| -> Result<String> {
            let partial = folder.join(format!("{filename}{PARTIAL_SUFFIX}"));
            let mut file = fs::File::create(&partial)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&partial, folder.join(&filename))?;
            Ok(to_hex(&Sha256::digest(bytes)))
        })();
        let sha256 = match result {
            Ok(hash) => hash,
            Err(e) => {
                let _ = fs::remove_dir_all(&folder);
                return Err(e);
            }
        };

        let attachment = Attachment {
            id,
            note_id: note_id.map(String::from),
            filename,
            mime,
            size: bytes.len() as u64,
            sha256,
            created_at: self.now(),
        };
        let inserted = self.conn.execute(
            &format!("INSERT INTO attachments ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"),
            params![
                attachment.id,
                attachment.note_id,
                attachment.filename,
                attachment.mime,
                attachment.size as i64,
                attachment.sha256,
                attachment.created_at
            ],
        );
        if let Err(e) = inserted {
            let _ = fs::remove_dir_all(&folder);
            return Err(e.into());
        }
        Ok(attachment)
    }

    /// Read a file from disk and attach it.
    pub fn add_attachment_from_file(
        &self,
        dir: &Path,
        note_id: Option<&str>,
        path: &Path,
    ) -> Result<Attachment> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let size = fs::metadata(path)?.len();
        if size > MAX_ATTACHMENT_BYTES {
            return Err(Error::invalid(format!(
                "“{}” is too large to attach (the limit is {} MB).",
                sanitize_filename(&name),
                MAX_ATTACHMENT_BYTES / 1024 / 1024
            )));
        }
        let bytes = fs::read(path)?;
        self.add_attachment(dir, note_id, &name, "", &bytes)
    }

    pub fn get_attachment(&self, id: &str) -> Result<Attachment> {
        if !is_valid_id(id) {
            return Err(Error::not_found("attachment", id));
        }
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM attachments WHERE id = ?1"),
                [id],
                attachment_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::not_found("attachment", id))
    }

    /// Where an attachment's file lives on disk.
    pub fn attachment_path(&self, dir: &Path, id: &str) -> Result<PathBuf> {
        let a = self.get_attachment(id)?;
        if !is_valid_id(&a.id) || !is_safe_filename(&a.filename) {
            return Err(Error::invalid("This attachment's record is damaged."));
        }
        Ok(dir.join(&a.id).join(&a.filename))
    }

    /// Fail if any attachment record has an invalid id or an unsafe file name (checked
    /// when restoring a backup, before it replaces the notes).
    pub fn check_attachment_records(&self) -> Result<()> {
        let bad = self
            .list_attachments()?
            .into_iter()
            .any(|a| !is_valid_id(&a.id) || !is_safe_filename(&a.filename));
        if bad {
            return Err(Error::InvalidBackup(
                "The backup contains invalid attachment records, so it wasn't restored.".into(),
            ));
        }
        Ok(())
    }

    pub fn list_attachments(&self) -> Result<Vec<Attachment>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM attachments ORDER BY created_at"
        ))?;
        let rows = stmt
            .query_map([], attachment_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Ids referenced by any note, including notes in Recently Deleted.
    pub fn referenced_attachment_ids(&self) -> Result<HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT content_json, locked_attachments FROM notes")?;
        let mut ids = HashSet::new();
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (json, locked) = row?;
            if let Ok(doc) = serde_json::from_str::<Value>(&json) {
                referenced_ids(&doc, &mut ids);
            }
            // Locked notes' documents are encrypted; their attachment ids are kept beside them.
            ids.extend(locked.split_whitespace().map(str::to_string));
        }
        Ok(ids)
    }

    /// Delete attachments no note refers to any more, once they're older than `grace_ms`
    /// (so undo keeps working for a while). Also removes stray folders left by crashes.
    /// Returns how many attachments were removed.
    pub fn collect_unused_attachments(
        &self,
        dir: &Path,
        now_ms: i64,
        grace_ms: i64,
    ) -> Result<usize> {
        let referenced = self.referenced_attachment_ids()?;
        let cutoff = now_ms - grace_ms;
        let mut removed = 0;
        let mut known = HashSet::new();
        for a in self.list_attachments()? {
            // Only ever delete folders named by a real attachment id inside `dir`.
            if !is_valid_id(&a.id) {
                continue;
            }
            if referenced.contains(&a.id) || a.created_at > cutoff {
                known.insert(a.id);
                continue;
            }
            self.conn
                .execute("DELETE FROM attachments WHERE id = ?1", [&a.id])?;
            match fs::remove_dir_all(dir.join(&a.id)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
            removed += 1;
        }
        // Folders with no database row (e.g. a crash between writing and recording).
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let old_enough = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .is_some_and(|d| (d.as_millis() as i64) <= cutoff);
                if is_valid_id(&name) && !known.contains(&name) && old_enough {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_are_made_safe() {
        assert_eq!(sanitize_filename("photo.png"), "photo.png");
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename(r"C:\Users\me\report.pdf"), "report.pdf");
        assert_eq!(sanitize_filename("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(sanitize_filename("  .hidden  "), "hidden");
        assert_eq!(sanitize_filename(""), "attachment");
        assert_eq!(sanitize_filename(".."), "attachment");
        assert_eq!(sanitize_filename("x.partial"), "x.partial_");
        let long = format!("{}.jpeg", "n".repeat(300));
        let short = sanitize_filename(&long);
        assert_eq!(short.chars().count(), 120);
        assert!(short.ends_with(".jpeg"));
    }

    #[test]
    fn long_names_fit_the_file_system_in_bytes() {
        for name in [
            format!("{}.pdf", "漢".repeat(100)), // 3 bytes per character
            format!("{}.docx", "й".repeat(150)), // 2 bytes per character
            "😀".repeat(80),                     // 4 bytes, no extension
        ] {
            let short = sanitize_filename(&name);
            assert!(short.len() <= MAX_FILENAME_BYTES, "{} bytes", short.len());
            assert!(
                short.len() + PARTIAL_SUFFIX.len() <= 143,
                "fits eCryptfs while written"
            );
            assert_eq!(sanitize_filename(&short), short, "stable");
            if let Some((_, ext)) = name.rsplit_once('.') {
                assert!(short.ends_with(&format!(".{ext}")), "{short}");
            }
        }
    }

    #[test]
    fn cleaning_a_clean_name_changes_nothing() {
        let tricky = [
            format!("{} b{}", "a".repeat(119), "c".repeat(20)), // cut right after a space
            format!("{}.{}", "a".repeat(119), "c".repeat(20)),  // cut right after a dot
            format!("{}   x.pdf", "a".repeat(115)),
            " . spaced . ".to_string(),
            "x.partial".to_string(),
            "a:b*c?.txt".to_string(),
        ];
        for name in tricky {
            let once = sanitize_filename(&name);
            assert_eq!(sanitize_filename(&once), once, "{name:?}");
            assert!(is_safe_filename(&once), "{once:?}");
            assert!(!once.ends_with(' ') && !once.ends_with('.'), "{once:?}");
        }
    }

    #[test]
    fn images_are_recognised_by_their_bytes() {
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"GIF89a..."), Some("image/gif"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff_image(b"\0\0\0\x1cftypavif...."), Some("image/avif"));
        assert_eq!(sniff_image(b"<html>not an image"), None);
        assert_eq!(sniff_image(b""), None);
    }

    #[test]
    fn ids_must_be_uuids() {
        assert!(is_valid_id(&Uuid::now_v7().to_string()));
        assert!(!is_valid_id("../secret"));
        assert!(!is_valid_id(""));
    }

    #[test]
    fn finds_referenced_ids_anywhere_in_a_document() {
        let doc = serde_json::json!({ "type": "doc", "content": [
            { "type": "image", "attrs": { "attachmentId": "a" } },
            { "type": "table", "content": [{ "type": "tableRow", "content": [{ "type": "tableCell", "content": [
                { "type": "attachment", "attrs": { "attachmentId": "b", "filename": "x.pdf" } }
            ]}]}]},
        ]});
        let mut ids = HashSet::new();
        referenced_ids(&doc, &mut ids);
        assert_eq!(ids, HashSet::from(["a".to_string(), "b".to_string()]));
    }
}
