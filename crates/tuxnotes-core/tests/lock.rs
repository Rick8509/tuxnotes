use serde_json::{Value, json};
use tuxnotes_core::attachments::UNUSED_GRACE_MS;
use tuxnotes_core::crypto::KdfParams;
use tuxnotes_core::{Error, NoteFilter, Store};

/// Cheap key-derivation costs, so tests run fast (the app uses `KdfParams::default()`).
const FAST: KdfParams = KdfParams {
    m_kib: 8 * 1024,
    t: 1,
    p: 1,
};

fn doc(lines: &[&str]) -> Value {
    let content: Vec<Value> = lines
        .iter()
        .map(|line| json!({ "type": "paragraph", "content": [{ "type": "text", "text": line }] }))
        .collect();
    json!({ "type": "doc", "content": content })
}

/// A store with a password set ("hunter22") and one locked note. Locked notes are unlocked.
fn store_with_locked_note() -> (Store, String) {
    let mut s = Store::open_in_memory().unwrap();
    let n = s.create_note(None).unwrap();
    s.update_note_content(&n.id, &doc(&["Bank details", "PIN 4321 for the blue card"]))
        .unwrap();
    s.set_lock_password_with("hunter22", "the usual", FAST)
        .unwrap();
    s.lock_note(&n.id).unwrap();
    (s, n.id)
}

fn raw(s: &Store, id: &str) -> (String, String, String, Option<Vec<u8>>) {
    s.conn_for_tests()
        .query_row(
            "SELECT content_json, content_text, snippet, locked_blob FROM notes WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
}

#[test]
fn locking_encrypts_the_body_and_keeps_the_title() {
    let (mut s, id) = store_with_locked_note();
    let (json, text, snippet, blob) = raw(&s, &id);
    assert!(!json.contains("4321") && !text.contains("4321") && snippet.is_empty());
    let blob = blob.expect("encrypted document stored");
    assert!(
        !blob.windows(4).any(|w| w == b"4321"),
        "body isn't readable in the database"
    );

    let summary = s.get_note_summary(&id).unwrap();
    assert!(summary.is_locked);
    assert_eq!(summary.title, "Bank details");
    assert!(
        s.search("4321", 10).unwrap().is_empty(),
        "body isn't searchable"
    );
    assert_eq!(s.search("bank", 10).unwrap().len(), 1, "title still is");

    // Still readable while unlocked.
    assert_eq!(
        s.get_note(&id).unwrap().content,
        doc(&["Bank details", "PIN 4321 for the blue card"])
    );
    // Locked: no content, no edits.
    s.lock_notes();
    let note = s.get_note(&id).unwrap();
    assert!(note.is_locked && note.content.is_null());
    assert!(matches!(
        s.update_note_content(&id, &doc(&["x"])),
        Err(Error::InvalidInput(_))
    ));
    assert!(!s.lock_status().unwrap().unlocked);
}

#[test]
fn unlocking_needs_the_right_password() {
    let (mut s, id) = store_with_locked_note();
    s.lock_notes();
    assert!(matches!(
        s.unlock_notes("hunter2"),
        Err(Error::WrongPassword)
    ));
    assert!(matches!(s.unlock_notes(""), Err(Error::WrongPassword)));
    assert!(s.get_note(&id).unwrap().content.is_null());
    s.unlock_notes("hunter22").unwrap();
    assert!(
        s.get_note(&id)
            .unwrap()
            .content
            .to_string()
            .contains("4321")
    );
    let status = s.lock_status().unwrap();
    assert!(status.password_set && status.unlocked);
    assert_eq!(
        (status.hint.as_str(), status.locked_notes),
        ("the usual", 1)
    );
}

#[test]
fn editing_a_locked_note_keeps_it_encrypted() {
    let (s, id) = store_with_locked_note();
    let before = s.get_note_summary(&id).unwrap().updated_at;
    s.update_note_content(&id, &doc(&["Bank (old)", "new PIN 9876"]))
        .unwrap();
    let (json, text, _, blob) = raw(&s, &id);
    assert!(
        !json.contains("9876")
            && !text.contains("9876")
            && !blob.unwrap().windows(4).any(|w| w == b"9876")
    );
    let summary = s.get_note_summary(&id).unwrap();
    assert_eq!(summary.title, "Bank (old)", "title follows the first line");
    assert!(summary.updated_at > before, "edits count as changes");
    assert!(
        s.get_note(&id)
            .unwrap()
            .content
            .to_string()
            .contains("9876")
    );
}

#[test]
fn locking_and_unlocking_a_note_doesnt_change_its_modified_time() {
    let (s, id) = store_with_locked_note();
    let t = s.get_note_summary(&id).unwrap().updated_at;
    s.remove_note_lock(&id).unwrap();
    s.lock_note(&id).unwrap();
    assert_eq!(s.get_note_summary(&id).unwrap().updated_at, t);
}

#[test]
fn removing_the_lock_restores_a_normal_note() {
    let (s, id) = store_with_locked_note();
    let summary = s.remove_note_lock(&id).unwrap();
    assert!(!summary.is_locked);
    assert_eq!(summary.snippet, "PIN 4321 for the blue card");
    let (json, _, _, blob) = raw(&s, &id);
    assert!(json.contains("4321") && blob.is_none());
    assert_eq!(s.search("4321", 10).unwrap().len(), 1);
    assert_eq!(s.lock_status().unwrap().locked_notes, 0);
}

#[test]
fn locking_needs_a_password_and_an_unlocked_session() {
    let mut s = Store::open_in_memory().unwrap();
    let n = s.create_note(None).unwrap();
    s.update_note_content(&n.id, &doc(&["Secret"])).unwrap();
    assert!(
        matches!(s.lock_note(&n.id), Err(Error::InvalidInput(m)) if m.contains("Set a password"))
    );
    s.set_lock_password_with("hunter22", "", FAST).unwrap();
    s.lock_notes();
    assert!(matches!(s.lock_note(&n.id), Err(Error::InvalidInput(m)) if m.contains("locked")));
    assert!(
        matches!(
            s.set_lock_password_with("other", "", FAST),
            Err(Error::InvalidInput(_))
        ),
        "only once"
    );
}

#[test]
fn passwords_and_hints_are_checked() {
    let mut s = Store::open_in_memory().unwrap();
    assert!(
        matches!(
            s.set_lock_password_with("abc", "", FAST),
            Err(Error::InvalidInput(_))
        ),
        "too short"
    );
    assert!(
        matches!(
            s.set_lock_password_with("Swordfish", "it's swordfish!", FAST),
            Err(Error::InvalidInput(_))
        ),
        "hint gives it away"
    );
    assert!(!s.lock_status().unwrap().password_set);
    s.set_lock_password_with("pässwörd 🔒", "  fish  ", FAST)
        .unwrap();
    s.lock_notes();
    s.unlock_notes("pässwörd 🔒").unwrap();
    assert_eq!(s.lock_status().unwrap().hint, "fish");
}

#[test]
fn changing_the_password_keeps_notes_readable() {
    let (mut s, id) = store_with_locked_note();
    assert!(matches!(
        s.change_lock_password_with("wrong", "newpass1", "", FAST),
        Err(Error::WrongPassword)
    ));
    s.change_lock_password_with("hunter22", "newpass1", "new hint", FAST)
        .unwrap();
    s.lock_notes();
    assert!(matches!(
        s.unlock_notes("hunter22"),
        Err(Error::WrongPassword)
    ));
    s.unlock_notes("newpass1").unwrap();
    assert!(
        s.get_note(&id)
            .unwrap()
            .content
            .to_string()
            .contains("4321")
    );
    assert_eq!(s.lock_status().unwrap().hint, "new hint");
}

#[test]
fn locked_notes_survive_reopening_but_the_key_doesnt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.db");
    let id = {
        let mut s = Store::open(&path).unwrap();
        let n = s.create_note(None).unwrap();
        s.update_note_content(&n.id, &doc(&["Diary", "dear diary"]))
            .unwrap();
        s.set_lock_password_with("hunter22", "", FAST).unwrap();
        s.lock_note(&n.id).unwrap();
        n.id
    };
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !bytes.windows(10).any(|w| w == b"dear diary"),
        "no plain text left in the file"
    );
    let mut s = Store::open(&path).unwrap();
    let status = s.lock_status().unwrap();
    assert!(
        status.password_set && !status.unlocked,
        "locked again after a restart"
    );
    assert!(s.get_note(&id).unwrap().content.is_null());
    s.unlock_notes("hunter22").unwrap();
    assert!(
        s.get_note(&id)
            .unwrap()
            .content
            .to_string()
            .contains("dear diary")
    );
}

#[test]
fn cleanup_never_removes_locked_notes_or_their_attachments() {
    let (mut s, id) = store_with_locked_note();
    let dir = tempfile::tempdir().unwrap();
    let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
    let a = s
        .add_attachment(dir.path(), Some(&id), "pic.png", "image/png", &png)
        .unwrap();
    let mut with_image = doc(&["Bank details"]);
    with_image["content"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "type": "image", "attrs": { "attachmentId": a.id } }));
    s.update_note_content(&id, &with_image).unwrap();
    s.lock_notes();

    assert_eq!(s.discard_blank_notes().unwrap(), 0);
    assert!(!s.discard_note_if_blank(&id).unwrap());
    let later = a.created_at + UNUSED_GRACE_MS + 1;
    assert_eq!(
        s.collect_unused_attachments(dir.path(), later, UNUSED_GRACE_MS)
            .unwrap(),
        0
    );
    assert!(s.get_attachment(&a.id).is_ok());
    assert_eq!(s.list_notes(&NoteFilter::All).unwrap().len(), 1);
}

#[test]
fn locked_notes_cant_be_duplicated_and_trash_keeps_them_locked() {
    let (mut s, id) = store_with_locked_note();
    assert!(matches!(s.duplicate_note(&id), Err(Error::InvalidInput(_))));
    s.trash_note(&id).unwrap();
    s.lock_notes();
    let trashed = s.list_notes(&NoteFilter::Trash).unwrap();
    assert!(trashed[0].is_locked);
    s.restore_note(&id).unwrap();
    assert!(s.get_note(&id).unwrap().content.is_null());
}

#[test]
fn damaged_or_swapped_ciphertext_is_an_error_not_a_crash() {
    let (s, id) = store_with_locked_note();
    let other = s.create_note(None).unwrap();
    s.update_note_content(&other.id, &doc(&["Other"])).unwrap();
    s.lock_note(&other.id).unwrap();
    // Copy note A's encrypted body onto note B: it's bound to A's id, so B can't open it.
    s.conn_for_tests()
        .execute("UPDATE notes SET locked_blob = (SELECT locked_blob FROM notes WHERE id = ?1) WHERE id = ?2", [&id, &other.id])
        .unwrap();
    assert!(matches!(s.get_note(&other.id), Err(Error::Crypto(_))));
    s.conn_for_tests()
        .execute(
            "UPDATE notes SET locked_blob = X'0102' WHERE id = ?1",
            [&id],
        )
        .unwrap();
    assert!(matches!(s.get_note(&id), Err(Error::Crypto(_))));
}

#[test]
fn backups_carry_locked_notes_and_the_password() {
    let (s, id) = store_with_locked_note();
    let dir = tempfile::tempdir().unwrap();
    let snap = dir.path().join("snap.db");
    s.snapshot_to(&snap).unwrap();
    let mut restored = Store::open(&snap).unwrap();
    assert!(restored.get_note(&id).unwrap().content.is_null());
    restored.unlock_notes("hunter22").unwrap();
    assert!(
        restored
            .get_note(&id)
            .unwrap()
            .content
            .to_string()
            .contains("4321")
    );
}

#[test]
fn the_real_password_settings_are_quick_enough() {
    let mut s = Store::open_in_memory().unwrap();
    let start = std::time::Instant::now();
    s.set_lock_password("hunter22", "").unwrap();
    s.lock_notes();
    s.unlock_notes("hunter22").unwrap();
    let each = start.elapsed() / 2;
    eprintln!("one password check with the default settings: {each:?}");
    assert!(each < std::time::Duration::from_secs(3), "{each:?}");
}
