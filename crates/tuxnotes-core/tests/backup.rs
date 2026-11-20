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

    let manifest = create_backup(&live, None, &archive, "0.1.0").unwrap();
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
    assert_eq!(
        extract_backup(&archive, &restored_path, None).unwrap(),
        manifest
    );
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
    create_backup(&live, None, &archive, "0.1.0").unwrap();
    assert!(matches!(
        create_backup(&live, None, &archive, "0.1.0"),
        Err(Error::InvalidInput(_))
    ));

    let existing = dir.path().join("existing.db");
    fs::write(&existing, b"keep me").unwrap();
    assert!(matches!(
        extract_backup(&archive, &existing, None),
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
        extract_backup(&other, &dir.path().join("r.db"), None),
        Err(Error::InvalidBackup(_))
    ));
}

#[test]
fn rejects_tampered_or_corrupt_databases_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let live = sample_store(dir.path());
    let good = dir.path().join("good.zip");
    create_backup(&live, None, &good, "0.1.0").unwrap();

    let tampered = dir.path().join("tampered.zip");
    rewrite(&good, &tampered, |_, db| {
        let mid = db.len() / 2;
        db[mid] ^= 0xff;
    });
    let dest = dir.path().join("r.db");
    let err = extract_backup(&tampered, &dest, None).unwrap_err();
    assert!(err.to_string().contains("checksum"), "{err}");
    assert!(!dest.exists(), "partial restore must be removed");

    // Checksum "fixed" but the bytes are garbage: must still be rejected.
    let garbage = dir.path().join("garbage.zip");
    rewrite(&good, &garbage, |m, db| {
        *db = b"definitely not sqlite".repeat(100);
        m["databaseSize"] = json!(db.len());
        m["databaseSha256"] = json!(sha256_hex(db));
    });
    assert!(extract_backup(&garbage, &dest, None).is_err());
    for p in backup::db_with_sidecars(&dest) {
        assert!(!p.exists(), "{} left behind", p.display());
    }
}

#[test]
fn rejects_backups_from_newer_versions() {
    let dir = tempfile::tempdir().unwrap();
    let live = sample_store(dir.path());
    let good = dir.path().join("good.zip");
    create_backup(&live, None, &good, "0.1.0").unwrap();

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
    create_backup(&live, None, &archive, "0.1.0").unwrap();
    // Changes after the backup must not appear in it.
    let n = live.create_note(None).unwrap();
    live.update_note_content(&n.id, &doc("After backup"))
        .unwrap();

    let restored_path = dir.path().join("r.db");
    extract_backup(&archive, &restored_path, None).unwrap();
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

#[test]
fn notes_with_tables_save_search_and_survive_backup() {
    let dir = tempfile::tempdir().unwrap();
    let live = Store::open(dir.path().join("live.db")).unwrap();
    let cell = |kind: &str, t: &str| {
        json!({ "type": kind, "attrs": { "colspan": 1, "rowspan": 1, "colwidth": [120] },
                "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": t }] }] })
    };
    let table_doc = json!({ "type": "doc", "content": [
        { "type": "paragraph", "content": [{ "type": "text", "text": "Milestones" }] },
        { "type": "table", "content": [
            { "type": "tableRow", "content": [cell("tableHeader", "Name"), cell("tableHeader", "Status")] },
            { "type": "tableRow", "content": [cell("tableCell", "Scaffold"), cell("tableCell", "Finished")] },
        ]},
    ]});
    let note = live.create_note(None).unwrap();
    let summary = live.update_note_content(&note.id, &table_doc).unwrap();
    assert_eq!(summary.title, "Milestones");
    assert_eq!(summary.snippet, "Name Status Scaffold Finished");
    assert_eq!(
        live.search("scaffold", 10).unwrap().len(),
        1,
        "text inside tables is searchable"
    );

    let archive = dir.path().join("b.zip");
    create_backup(&live, None, &archive, "0.1.0").unwrap();
    let restored_path = dir.path().join("restored.db");
    extract_backup(&archive, &restored_path, None).unwrap();
    let restored = Store::open(&restored_path).unwrap();
    assert_eq!(
        restored.get_note(&note.id).unwrap().content,
        table_doc,
        "table (incl. column widths) restored exactly"
    );
    assert_eq!(restored.search("finished", 10).unwrap().len(), 1);
}

#[test]
fn notes_can_be_saved_while_the_archive_is_written() {
    use tuxnotes_core::backup::{snapshot_for_backup, write_backup};
    let dir = tempfile::tempdir().unwrap();
    let store = sample_store(dir.path());
    let dest = dir.path().join("b.zip");
    let note = store.list_notes(&NoteFilter::All).unwrap()[0].clone();

    // The store is only needed for the snapshot; edits after it don't end up in this backup.
    let snapshot = snapshot_for_backup(&store, &dest).unwrap();
    store
        .update_note_content(&note.id, &doc("edited after the snapshot"))
        .unwrap();
    let manifest = write_backup(snapshot, None, &dest, "test").unwrap();

    let restored_db = dir.path().join("restored.db");
    extract_backup(&dest, &restored_db, None).unwrap();
    let restored = Store::open(&restored_db).unwrap();
    assert_eq!(restored.get_note(&note.id).unwrap().title, note.title);
    assert_eq!(manifest.note_count, store.counts().unwrap().1);
    assert!(
        !dir.path().join("b.zip.snapshot.db").exists(),
        "the copy is removed"
    );
}

#[test]
fn pictures_are_stored_as_they_are_and_text_is_compressed() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("att");
    fs::create_dir_all(&att).unwrap();
    let store = Store::open_in_memory().unwrap();
    let n = store.create_note(None).unwrap();
    let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];
    let pic = store
        .add_attachment(&att, Some(&n.id), "photo.png", "image/png", &png)
        .unwrap();
    let txt = store
        .add_attachment(&att, Some(&n.id), "notes.txt", "text/plain", &[b'a'; 4000])
        .unwrap();
    let content = json!({ "type": "doc", "content": [
        { "type": "image", "attrs": { "attachmentId": pic.id } },
        { "type": "attachment", "attrs": { "attachmentId": txt.id } },
    ] });
    store.update_note_content(&n.id, &content).unwrap();
    let dest = dir.path().join("b.zip");
    create_backup(&store, Some(&att), &dest, "test").unwrap();

    let mut zip = zip::ZipArchive::new(File::open(&dest).unwrap()).unwrap();
    let method =
        |zip: &mut zip::ZipArchive<File>, name: String| zip.by_name(&name).unwrap().compression();
    assert_eq!(
        method(&mut zip, format!("attachments/{}/photo.png", pic.id)),
        zip::CompressionMethod::Stored
    );
    assert_eq!(
        method(&mut zip, format!("attachments/{}/notes.txt", txt.id)),
        zip::CompressionMethod::Deflated
    );
}
