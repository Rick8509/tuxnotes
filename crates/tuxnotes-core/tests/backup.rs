use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use serde_json::json;
use tuxnotes_core::backup::{
    self, BACKUP_FORMAT_VERSION, DATABASE_FILE, MANIFEST_FILE, create_backup, extract_backup,
    prune_local_backups, read_manifest,
};
use tuxnotes_core::{Error, NoteFilter, Store};
use zip::write::SimpleFileOptions;

fn doc(text: &str) -> serde_json::Value {
    json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }] })
}

/// A store with a folder, two notes and one trashed note.
fn sample_store(dir: &Path) -> Store {
    let s = Store::open(dir.join("live.db")).unwrap();
    let work = s.create_folder("Work").unwrap();
    let a = s.create_note(None).unwrap();
    s.update_note_content(&a.id, &doc("Groceries")).unwrap();
    let b = s.create_note(Some(&work.id)).unwrap();
    s.update_note_content(&b.id, &doc("Standup")).unwrap();
    let c = s.create_note(None).unwrap();
    s.trash_note(&c.id).unwrap();
    s
}

/// Rewrite an archive, letting `edit` change the manifest JSON and/or database bytes.
fn rewrite(src: &Path, dest: &Path, edit: impl FnOnce(&mut serde_json::Value, &mut Vec<u8>)) {
    let mut zip = zip::ZipArchive::new(File::open(src).unwrap()).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_reader(zip.by_name(MANIFEST_FILE).unwrap()).unwrap();
    let mut db = Vec::new();
    std::io::copy(&mut zip.by_name(DATABASE_FILE).unwrap(), &mut db).unwrap();
    edit(&mut manifest, &mut db);
    let mut out = zip::ZipWriter::new(File::create(dest).unwrap());
    out.start_file(MANIFEST_FILE, SimpleFileOptions::default())
        .unwrap();
    out.write_all(manifest.to_string().as_bytes()).unwrap();
    out.start_file(DATABASE_FILE, SimpleFileOptions::default())
        .unwrap();
    out.write_all(&db).unwrap();
    out.finish().unwrap();
}

#[test]
fn backup_roundtrip_restores_everything() {
    let dir = tempfile::tempdir().unwrap();
    let live = sample_store(dir.path());
    let archive = dir.path().join("b.zip");

    let manifest = create_backup(&live, &archive, "0.1.0").unwrap();
    assert_eq!(manifest.app, "TuxNotes");
    assert_eq!(manifest.format_version, BACKUP_FORMAT_VERSION);
    assert_eq!(manifest.app_version, "0.1.0");
    assert_eq!((manifest.folder_count, manifest.note_count), (2, 3));
    assert_eq!(manifest.database_sha256.len(), 64);
    assert_eq!(read_manifest(&archive).unwrap(), manifest);

    // No temporary files left behind.
    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.contains("partial") || n.contains("snapshot"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    let restored_path = dir.path().join("restored.db");
    assert_eq!(extract_backup(&archive, &restored_path).unwrap(), manifest);
    let restored = Store::open(&restored_path).unwrap();
    let titles: Vec<_> = restored
        .list_notes(&NoteFilter::All)
        .unwrap()
        .into_iter()
        .map(|n| n.title)
        .collect();
    assert_eq!(titles, ["Standup", "Groceries"]);
    assert_eq!(restored.trash_count().unwrap(), 1);
    assert_eq!(restored.list_folders().unwrap().len(), 2);
    assert_eq!(
        restored.search("groc", 10).unwrap().len(),
        1,
        "search index restored"
    );
}

#[test]
fn backup_never_overwrites_and_restore_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let live = sample_store(dir.path());
    let archive = dir.path().join("b.zip");
    create_backup(&live, &archive, "0.1.0").unwrap();
    assert!(matches!(
        create_backup(&live, &archive, "0.1.0"),
        Err(Error::InvalidInput(_))
    ));

    let existing = dir.path().join("existing.db");
    fs::write(&existing, b"keep me").unwrap();
    assert!(matches!(
        extract_backup(&archive, &existing),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(fs::read(&existing).unwrap(), b"keep me");
}

#[test]
fn rejects_files_that_are_not_backups() {
    let dir = tempfile::tempdir().unwrap();
    let not_zip = dir.path().join("x.zip");
    fs::write(&not_zip, b"hello").unwrap();
    assert!(matches!(
        read_manifest(&not_zip),
        Err(Error::InvalidBackup(_))
    ));

    // A zip, but without our manifest.
    let other = dir.path().join("other.zip");
    let mut z = zip::ZipWriter::new(File::create(&other).unwrap());
    z.start_file("readme.txt", SimpleFileOptions::default())
        .unwrap();
    z.write_all(b"hi").unwrap();
    z.finish().unwrap();
    assert!(matches!(
        read_manifest(&other),
        Err(Error::InvalidBackup(_))
    ));
    assert!(matches!(
        extract_backup(&other, &dir.path().join("r.db")),
        Err(Error::InvalidBackup(_))
    ));
}

#[test]
fn rejects_tampered_or_corrupt_databases_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let live = sample_store(dir.path());
    let good = dir.path().join("good.zip");
    create_backup(&live, &good, "0.1.0").unwrap();

    let tampered = dir.path().join("tampered.zip");
    rewrite(&good, &tampered, |_, db| {
        let mid = db.len() / 2;
        db[mid] ^= 0xff;
    });
    let dest = dir.path().join("r.db");
    let err = extract_backup(&tampered, &dest).unwrap_err();
    assert!(err.to_string().contains("checksum"), "{err}");
    assert!(!dest.exists(), "partial restore must be removed");

    // Checksum "fixed" but the bytes are garbage: must still be rejected.
    let garbage = dir.path().join("garbage.zip");
    rewrite(&good, &garbage, |m, db| {
        *db = b"definitely not sqlite".repeat(100);
        m["databaseSize"] = json!(db.len());
        m["databaseSha256"] = json!(sha256_hex(db));
    });
    assert!(extract_backup(&garbage, &dest).is_err());
    for p in backup::db_with_sidecars(&dest) {
        assert!(!p.exists(), "{} left behind", p.display());
    }
}

#[test]
fn rejects_backups_from_newer_versions() {
    let dir = tempfile::tempdir().unwrap();
    let live = sample_store(dir.path());
    let good = dir.path().join("good.zip");
    create_backup(&live, &good, "0.1.0").unwrap();

    for (field, value) in [("formatVersion", 99), ("schemaVersion", 99)] {
        let newer = dir.path().join(format!("newer-{field}.zip"));
        rewrite(&good, &newer, |m, _| m[field] = json!(value));
        let err = read_manifest(&newer).unwrap_err();
        assert!(err.to_string().contains("newer version"), "{err}");
    }
}

#[test]
fn backup_is_consistent_while_store_keeps_changing() {
    let dir = tempfile::tempdir().unwrap();
    let live = sample_store(dir.path());
    let archive = dir.path().join("b.zip");
    create_backup(&live, &archive, "0.1.0").unwrap();
    // Changes after the backup must not appear in it.
    let n = live.create_note(None).unwrap();
    live.update_note_content(&n.id, &doc("After backup"))
        .unwrap();

    let restored_path = dir.path().join("r.db");
    extract_backup(&archive, &restored_path).unwrap();
    let restored = Store::open(&restored_path).unwrap();
    assert!(restored.search("after", 10).unwrap().is_empty());
}

#[test]
fn prune_keeps_newest_matching_files_only() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "safety-20261101-000000.zip",
        "safety-20261103-000000.zip",
        "safety-20261102-000000.zip",
        "other.zip",
        "safety-notes.txt",
    ] {
        fs::write(dir.path().join(name), b"x").unwrap();
    }
    assert_eq!(prune_local_backups(dir.path(), "safety-", 2).unwrap(), 1);
    let mut left: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            "other.zip",
            "safety-20261102-000000.zip",
            "safety-20261103-000000.zip",
            "safety-notes.txt"
        ]
    );
    assert_eq!(
        prune_local_backups(&dir.path().join("missing"), "x", 1).unwrap(),
        0
    );
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
