use std::fs::{self, File};
use std::io::Write;

use serde_json::{Value, json};
use tuxnotes_core::attachments::{MAX_ATTACHMENT_BYTES, UNUSED_GRACE_MS};
use tuxnotes_core::backup::{self, create_backup, extract_backup};
use tuxnotes_core::{Error, Store};
use zip::write::SimpleFileOptions;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDRfake-image-bytes";

fn doc_with(nodes: Vec<Value>) -> Value {
    let mut content =
        vec![json!({ "type": "paragraph", "content": [{ "type": "text", "text": "Note" }] })];
    content.extend(nodes);
    json!({ "type": "doc", "content": content })
}

#[test]
fn adding_detects_images_and_stores_files_safely() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    let s = Store::open(dir.path().join("notes.db")).unwrap();
    let note = s.create_note(None).unwrap();

    // Claimed as a file, but the bytes are a PNG → treated as an image.
    let img = s
        .add_attachment(
            &att,
            Some(&note.id),
            "../../shot.png",
            "application/octet-stream",
            PNG,
        )
        .unwrap();
    assert_eq!(img.mime, "image/png");
    assert!(img.is_image());
    assert_eq!(img.filename, "shot.png", "path components are stripped");
    assert_eq!(img.size, PNG.len() as u64);
    let path = s.attachment_path(&att, &img.id).unwrap();
    assert_eq!(path, att.join(&img.id).join("shot.png"));
    assert_eq!(fs::read(&path).unwrap(), PNG);

    // Claimed as an image, but it isn't one → a plain file, typed by its name.
    let fake = s
        .add_attachment(
            &att,
            Some(&note.id),
            "virus.png",
            "image/png",
            b"<html><script>",
        )
        .unwrap();
    assert!(!fake.is_image());
    let pdf = s
        .add_attachment(&att, None, "report.pdf", "", b"%PDF-1.7")
        .unwrap();
    assert_eq!(pdf.mime, "application/pdf");

    assert_eq!(s.get_attachment(&img.id).unwrap(), img);
    assert_eq!(s.list_attachments().unwrap().len(), 3);
    assert!(matches!(
        s.get_attachment("../etc"),
        Err(Error::NotFound { .. })
    ));
    // No leftover temporary files.
    assert!(!att.join(&img.id).join("shot.png.partial").exists());
}

#[test]
fn rejects_files_that_are_too_large_or_for_missing_notes() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    let s = Store::open_in_memory().unwrap();
    let big = vec![0u8; (MAX_ATTACHMENT_BYTES + 1) as usize];
    let err = s
        .add_attachment(&att, None, "big.bin", "", &big)
        .unwrap_err();
    assert!(err.to_string().contains("too large"), "{err}");
    assert!(matches!(
        s.add_attachment(&att, Some("no-such-note"), "a.txt", "", b"x"),
        Err(Error::NotFound { .. })
    ));
    assert!(s.list_attachments().unwrap().is_empty());
    assert!(
        fs::read_dir(&att).map(|d| d.count()).unwrap_or(0) == 0,
        "nothing written"
    );
}

#[test]
fn unused_attachments_are_cleaned_up_after_the_grace_period() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    let s = Store::open(dir.path().join("notes.db")).unwrap();
    let note = s.create_note(None).unwrap();
    let kept = s
        .add_attachment(&att, Some(&note.id), "kept.png", "", PNG)
        .unwrap();
    let in_trash = s
        .add_attachment(&att, Some(&note.id), "trash.png", "", PNG)
        .unwrap();
    let removed = s
        .add_attachment(&att, Some(&note.id), "removed.png", "", PNG)
        .unwrap();

    s.update_note_content(
        &note.id,
        &doc_with(vec![
            json!({ "type": "image", "attrs": { "attachmentId": kept.id } }),
        ]),
    )
    .unwrap();
    let trashed = s.create_note(None).unwrap();
    s.update_note_content(
        &trashed.id,
        &doc_with(vec![json!({ "type": "attachment", "attrs": { "attachmentId": in_trash.id, "filename": "trash.png" } })]),
    )
    .unwrap();
    s.trash_note(&trashed.id).unwrap();

    // Within the grace period nothing is removed (undo still works).
    assert_eq!(
        s.collect_unused_attachments(&att, removed.created_at, UNUSED_GRACE_MS)
            .unwrap(),
        0
    );
    // After it, only the unreferenced one goes; notes in Recently Deleted still keep theirs.
    let later = removed.created_at + UNUSED_GRACE_MS + 1;
    assert_eq!(
        s.collect_unused_attachments(&att, later, UNUSED_GRACE_MS)
            .unwrap(),
        1
    );
    assert!(!att.join(&removed.id).exists());
    assert!(att.join(&kept.id).exists() && att.join(&in_trash.id).exists());

    // Emptying the trash makes its attachment unused too.
    s.empty_trash().unwrap();
    assert_eq!(
        s.collect_unused_attachments(&att, later, UNUSED_GRACE_MS)
            .unwrap(),
        1
    );
    assert_eq!(s.list_attachments().unwrap().len(), 1);
}

#[test]
fn backups_include_attachments_and_restore_them() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    let s = Store::open(dir.path().join("notes.db")).unwrap();
    let note = s.create_note(None).unwrap();
    let img = s
        .add_attachment(&att, Some(&note.id), "photo.png", "", PNG)
        .unwrap();
    let file = s
        .add_attachment(&att, Some(&note.id), "plan.pdf", "", b"%PDF-1.7 data")
        .unwrap();
    s.update_note_content(
        &note.id,
        &doc_with(vec![
            json!({ "type": "image", "attrs": { "attachmentId": img.id } }),
            json!({ "type": "attachment", "attrs": { "attachmentId": file.id, "filename": "plan.pdf", "size": 13 } }),
        ]),
    )
    .unwrap();

    let archive = dir.path().join("b.zip");
    let manifest = create_backup(&s, Some(&att), &archive, "0.2.0").unwrap();
    assert_eq!(manifest.format_version, 2);
    assert_eq!(manifest.attachments.len(), 2);

    let db = dir.path().join("restored.db");
    let restored_att = dir.path().join("restored-attachments");
    extract_backup(&archive, &db, Some(&restored_att)).unwrap();
    assert_eq!(
        fs::read(restored_att.join(&img.id).join("photo.png")).unwrap(),
        PNG
    );
    assert_eq!(
        fs::read(restored_att.join(&file.id).join("plan.pdf")).unwrap(),
        b"%PDF-1.7 data"
    );
    let r = Store::open(&db).unwrap();
    assert_eq!(r.get_attachment(&img.id).unwrap().mime, "image/png");
    assert_eq!(
        r.search("plan", 10).unwrap().len(),
        1,
        "attached file names are searchable"
    );
}

#[test]
fn old_backups_without_attachments_still_restore() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("notes.db")).unwrap();
    s.create_note(None).unwrap();
    let archive = dir.path().join("b.zip");
    create_backup(&s, None, &archive, "0.1.0").unwrap();
    // Make it look exactly like a format-1 backup (no "attachments" key at all).
    rewrite_manifest(&archive, |m| {
        m["formatVersion"] = json!(1);
        m.as_object_mut().unwrap().remove("attachments");
    });
    let att = dir.path().join("att");
    let manifest = extract_backup(&archive, &dir.path().join("r.db"), Some(&att)).unwrap();
    assert!(manifest.attachments.is_empty());
    assert!(att.is_dir(), "an empty attachments folder is created");
}

#[test]
fn tampered_or_unsafe_attachments_are_rejected_and_cleaned_up() {
    let dir = tempfile::tempdir().unwrap();
    let att = dir.path().join("attachments");
    let s = Store::open(dir.path().join("notes.db")).unwrap();
    s.add_attachment(&att, None, "a.png", "", PNG).unwrap();
    let archive = dir.path().join("b.zip");
    create_backup(&s, Some(&att), &archive, "0.2.0").unwrap();

    let tampered = dir.path().join("tampered.zip");
    fs::copy(&archive, &tampered).unwrap();
    rewrite_manifest(&tampered, |m| {
        m["attachments"][0]["sha256"] = json!("0".repeat(64))
    });
    let (db, out) = (dir.path().join("r.db"), dir.path().join("out"));
    let err = extract_backup(&tampered, &db, Some(&out)).unwrap_err();
    assert!(err.to_string().contains("checksum"), "{err}");
    assert!(!out.exists() && !db.exists(), "nothing left behind");

    let evil = dir.path().join("evil.zip");
    fs::copy(&archive, &evil).unwrap();
    rewrite_manifest(&evil, |m| {
        m["attachments"][0]["path"] = json!("../../escape.sh")
    });
    let err = extract_backup(&evil, &db, Some(&out)).unwrap_err();
    assert!(err.to_string().contains("unsafe"), "{err}");
    assert!(!dir.path().join("escape.sh").exists());
}

/// Rewrite an archive's manifest, keeping every other entry.
fn rewrite_manifest(path: &std::path::Path, edit: impl FnOnce(&mut Value)) {
    let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
    let mut entries = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut buf = Vec::new();
        std::io::copy(&mut f, &mut buf).unwrap();
        entries.push((f.name().unwrap().into_owned(), buf));
    }
    let mut edit = Some(edit);
    let tmp = path.with_extension("tmp");
    let mut out = zip::ZipWriter::new(File::create(&tmp).unwrap());
    for (name, mut data) in entries {
        if name == backup::MANIFEST_FILE {
            let mut m: Value = serde_json::from_slice(&data).unwrap();
            (edit.take().expect("one manifest"))(&mut m);
            data = serde_json::to_vec(&m).unwrap();
        }
        out.start_file(name, SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap();
    fs::rename(tmp, path).unwrap();
}
