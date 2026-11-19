use serde_json::{Value, json};
use tuxnotes_core::store::{DEFAULT_FOLDER_NAME, TRASH_RETENTION_DAYS};
use tuxnotes_core::{Error, NoteFilter, Store};

fn doc(lines: &[&str]) -> Value {
    let content: Vec<Value> = lines
        .iter()
        .map(|line| json!({ "type": "paragraph", "content": [{ "type": "text", "text": line }] }))
        .collect();
    json!({ "type": "doc", "content": content })
}

fn ids(list: &[tuxnotes_core::NoteSummary]) -> Vec<&str> {
    list.iter().map(|n| n.id.as_str()).collect()
}

fn store() -> Store {
    Store::open_in_memory().expect("open store")
}

// ----------------------------------------------------------------- folders

#[test]
fn new_store_has_only_the_default_folder() {
    let s = store();
    let folders = s.list_folders().unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].name, DEFAULT_FOLDER_NAME);
    assert!(folders[0].is_default);
    assert_eq!(folders[0].note_count, 0);
}

#[test]
fn folders_are_created_in_order_and_names_are_trimmed() {
    let s = store();
    s.create_folder("  Work ").unwrap();
    s.create_folder("Ideas").unwrap();
    let names: Vec<_> = s
        .list_folders()
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(names, [DEFAULT_FOLDER_NAME, "Work", "Ideas"]);
}

#[test]
fn folder_names_must_be_valid_and_unique() {
    let s = store();
    s.create_folder("Work").unwrap();
    assert!(matches!(
        s.create_folder("   "),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        s.create_folder("work"),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        s.create_folder("NOTES"),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        s.create_folder(&"x".repeat(101)),
        Err(Error::InvalidInput(_))
    ));
    // Unicode-aware case folding.
    s.create_folder("Ärger").unwrap();
    assert!(matches!(
        s.create_folder("ärger"),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn rename_folder_allows_same_name_with_different_case() {
    let s = store();
    let f = s.create_folder("work").unwrap();
    assert_eq!(s.rename_folder(&f.id, "Work").unwrap().name, "Work");
}

#[test]
fn default_folder_cannot_be_renamed_or_deleted() {
    let mut s = store();
    let default_id = s.default_folder_id().unwrap();
    assert!(matches!(
        s.rename_folder(&default_id, "Other"),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        s.delete_folder(&default_id),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn deleting_a_folder_moves_its_notes_to_trash_and_restore_puts_them_in_default() {
    let mut s = store();
    let work = s.create_folder("Work").unwrap();
    let note = s.create_note(Some(&work.id)).unwrap();
    s.update_note_content(&note.id, &doc(&["Meeting"])).unwrap();
    s.set_note_pinned(&note.id, true).unwrap();

    assert_eq!(s.delete_folder(&work.id).unwrap(), 1);
    assert!(matches!(
        s.get_folder(&work.id),
        Err(Error::NotFound { .. })
    ));

    let trashed = s.get_note_summary(&note.id).unwrap();
    assert!(trashed.deleted_at.is_some());
    assert!(!trashed.is_pinned);

    let restored = s.restore_note(&note.id).unwrap();
    assert_eq!(restored.folder_id, s.default_folder_id().unwrap());
    assert_eq!(restored.title, "Meeting");
}

#[test]
fn folder_note_counts_exclude_trash() {
    let s = store();
    let a = s.create_note(None).unwrap();
    s.create_note(None).unwrap();
    s.trash_note(&a.id).unwrap();
    assert_eq!(s.list_folders().unwrap()[0].note_count, 1);
    assert_eq!(s.trash_count().unwrap(), 1);
}

// ------------------------------------------------------------------- notes

#[test]
fn create_note_in_default_or_given_folder() {
    let s = store();
    let work = s.create_folder("Work").unwrap();
    let n1 = s.create_note(None).unwrap();
    let n2 = s.create_note(Some(&work.id)).unwrap();
    assert_eq!(n1.folder_id, s.default_folder_id().unwrap());
    assert_eq!(n2.folder_id, work.id);
    assert_eq!(n1.title, "");
    assert_eq!(n1.content["type"], "doc");
    assert!(matches!(
        s.create_note(Some("nope")),
        Err(Error::NotFound { .. })
    ));
}

#[test]
fn updating_content_derives_title_and_snippet_and_roundtrips() {
    let s = store();
    let n = s.create_note(None).unwrap();
    let content = doc(&["Shopping list", "milk", "bread"]);
    let summary = s.update_note_content(&n.id, &content).unwrap();
    assert_eq!(summary.title, "Shopping list");
    assert_eq!(summary.snippet, "milk bread");
    assert!(summary.updated_at > n.updated_at);

    let loaded = s.get_note(&n.id).unwrap();
    assert_eq!(loaded.content, content);
}

#[test]
fn rejects_content_that_is_not_a_document() {
    let s = store();
    let n = s.create_note(None).unwrap();
    for bad in [json!(null), json!("text"), json!({ "type": "paragraph" })] {
        assert!(matches!(
            s.update_note_content(&n.id, &bad),
            Err(Error::InvalidInput(_))
        ));
    }
    assert!(matches!(
        s.update_note_content("missing", &doc(&["x"])),
        Err(Error::NotFound { .. })
    ));
}

#[test]
fn list_orders_pinned_first_then_most_recently_edited() {
    let s = store();
    let a = s.create_note(None).unwrap();
    let b = s.create_note(None).unwrap();
    let c = s.create_note(None).unwrap();
    assert_eq!(
        ids(&s.list_notes(&NoteFilter::All).unwrap()),
        [&c.id, &b.id, &a.id]
    );

    s.update_note_content(&a.id, &doc(&["edited"])).unwrap();
    assert_eq!(
        ids(&s.list_notes(&NoteFilter::All).unwrap()),
        [&a.id, &c.id, &b.id]
    );

    s.set_note_pinned(&b.id, true).unwrap();
    assert_eq!(
        ids(&s.list_notes(&NoteFilter::All).unwrap()),
        [&b.id, &a.id, &c.id]
    );
}

#[test]
fn folder_filter_only_shows_that_folder() {
    let s = store();
    let work = s.create_folder("Work").unwrap();
    let home_note = s.create_note(None).unwrap();
    let work_note = s.create_note(Some(&work.id)).unwrap();

    let work_list = s
        .list_notes(&NoteFilter::Folder {
            folder_id: work.id.clone(),
        })
        .unwrap();
    assert_eq!(ids(&work_list), [&work_note.id]);

    s.move_note(&home_note.id, &work.id).unwrap();
    let work_list = s
        .list_notes(&NoteFilter::Folder {
            folder_id: work.id.clone(),
        })
        .unwrap();
    assert_eq!(work_list.len(), 2);

    assert!(matches!(
        s.list_notes(&NoteFilter::Folder {
            folder_id: "nope".into()
        }),
        Err(Error::NotFound { .. })
    ));
}

// ------------------------------------------------------------------- trash

#[test]
fn trashed_notes_are_read_only_until_restored() {
    let s = store();
    let n = s.create_note(None).unwrap();
    s.trash_note(&n.id).unwrap();

    assert!(s.list_notes(&NoteFilter::All).unwrap().is_empty());
    assert_eq!(ids(&s.list_notes(&NoteFilter::Trash).unwrap()), [&n.id]);
    assert!(matches!(
        s.update_note_content(&n.id, &doc(&["x"])),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        s.set_note_pinned(&n.id, true),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(s.trash_note(&n.id), Err(Error::InvalidInput(_))));

    s.restore_note(&n.id).unwrap();
    assert!(matches!(s.restore_note(&n.id), Err(Error::InvalidInput(_))));
    s.update_note_content(&n.id, &doc(&["back"])).unwrap();
}

#[test]
fn permanent_delete_requires_trash_first() {
    let s = store();
    let n = s.create_note(None).unwrap();
    assert!(matches!(
        s.delete_note_permanently(&n.id),
        Err(Error::InvalidInput(_))
    ));
    s.trash_note(&n.id).unwrap();
    s.delete_note_permanently(&n.id).unwrap();
    assert!(matches!(s.get_note(&n.id), Err(Error::NotFound { .. })));
}

#[test]
fn empty_trash_removes_only_trashed_notes() {
    let s = store();
    let keep = s.create_note(None).unwrap();
    for _ in 0..3 {
        let n = s.create_note(None).unwrap();
        s.trash_note(&n.id).unwrap();
    }
    assert_eq!(s.empty_trash().unwrap(), 3);
    assert_eq!(ids(&s.list_notes(&NoteFilter::All).unwrap()), [&keep.id]);
}

#[test]
fn purge_removes_only_expired_trash() {
    let s = store();
    let n = s.create_note(None).unwrap();
    let deleted_at = s.trash_note(&n.id).unwrap().deleted_at.unwrap();
    let day = 24 * 60 * 60 * 1000;

    let just_before = deleted_at + TRASH_RETENTION_DAYS * day - 1;
    assert_eq!(
        s.purge_trash_older_than(just_before, TRASH_RETENTION_DAYS)
            .unwrap(),
        0
    );

    let expiry = deleted_at + TRASH_RETENTION_DAYS * day;
    assert_eq!(
        s.purge_trash_older_than(expiry, TRASH_RETENTION_DAYS)
            .unwrap(),
        1
    );
    assert_eq!(s.purge_expired_trash().unwrap(), 0);
}

// ------------------------------------------------------------------ search

#[test]
fn search_matches_prefixes_in_title_and_body() {
    let s = store();
    let a = s.create_note(None).unwrap();
    let b = s.create_note(None).unwrap();
    s.update_note_content(&a.id, &doc(&["Groceries", "milk and eggs"]))
        .unwrap();
    s.update_note_content(&b.id, &doc(&["Ideas", "build a café app"]))
        .unwrap();

    assert_eq!(ids(&s.search("gro", 50).unwrap()), [&a.id]);
    assert_eq!(ids(&s.search("gro mil", 50).unwrap()), [&a.id]);
    assert_eq!(
        ids(&s.search("cafe", 50).unwrap()),
        [&b.id],
        "diacritics are ignored"
    );
    assert!(
        s.search("gro ideas", 50).unwrap().is_empty(),
        "all words must match"
    );
    assert!(s.search("   ", 50).unwrap().is_empty());
}

#[test]
fn search_ranks_title_matches_higher() {
    let s = store();
    let body_hit = s.create_note(None).unwrap();
    let title_hit = s.create_note(None).unwrap();
    s.update_note_content(&body_hit.id, &doc(&["Misc", "remember the budget"]))
        .unwrap();
    s.update_note_content(&title_hit.id, &doc(&["Budget", "numbers"]))
        .unwrap();
    assert_eq!(
        ids(&s.search("budget", 50).unwrap()),
        [&title_hit.id, &body_hit.id]
    );
}

#[test]
fn search_reflects_edits_and_skips_trash() {
    let s = store();
    let n = s.create_note(None).unwrap();
    s.update_note_content(&n.id, &doc(&["alpha"])).unwrap();
    s.update_note_content(&n.id, &doc(&["beta"])).unwrap();
    assert!(s.search("alpha", 50).unwrap().is_empty());
    assert_eq!(s.search("beta", 50).unwrap().len(), 1);

    s.trash_note(&n.id).unwrap();
    assert!(s.search("beta", 50).unwrap().is_empty());
    s.trash_note(&s.create_note(None).unwrap().id).unwrap();
    s.empty_trash().unwrap();
    assert!(s.search("beta", 50).unwrap().is_empty());
}

#[test]
fn search_is_safe_against_fts_syntax() {
    let s = store();
    for q in [
        "\"",
        "AND",
        "OR NOT",
        "a*b",
        "NEAR(",
        "(",
        "col:x",
        "^",
        "'; DROP TABLE notes;--",
    ] {
        s.search(q, 50)
            .unwrap_or_else(|e| panic!("query {q:?} failed: {e}"));
    }
}

#[test]
fn search_respects_limit() {
    let s = store();
    for _ in 0..5 {
        let n = s.create_note(None).unwrap();
        s.update_note_content(&n.id, &doc(&["same"])).unwrap();
    }
    assert_eq!(s.search("same", 3).unwrap().len(), 3);
}

// ------------------------------------------------- persistence & snapshots

#[test]
fn data_persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.db");
    let note_id;
    {
        let s = Store::open(&path).unwrap();
        let n = s.create_note(None).unwrap();
        s.update_note_content(&n.id, &doc(&["persisted"])).unwrap();
        note_id = n.id;
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(
        s.list_folders().unwrap().len(),
        1,
        "default folder not duplicated"
    );
    assert_eq!(s.get_note(&note_id).unwrap().title, "persisted");
    assert_eq!(s.search("persist", 10).unwrap().len(), 1);
}

#[test]
fn snapshot_is_a_complete_openable_copy() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("notes.db")).unwrap();
    let n = s.create_note(None).unwrap();
    s.update_note_content(&n.id, &doc(&["snapshot me"]))
        .unwrap();

    let snap = dir.path().join("snap.db");
    s.snapshot_to(&snap).unwrap();
    assert!(
        matches!(s.snapshot_to(&snap), Err(Error::InvalidInput(_))),
        "never overwrites"
    );

    let copy = Store::open(&snap).unwrap();
    assert_eq!(copy.get_note(&n.id).unwrap().title, "snapshot me");
    assert_eq!(
        copy.search("snap", 10).unwrap().len(),
        1,
        "FTS index included"
    );
}

#[test]
fn refuses_databases_from_newer_versions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.db");
    drop(Store::open(&path).unwrap());
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
    }
    assert!(matches!(
        Store::open(&path),
        Err(Error::UnsupportedSchema { found: 999, .. })
    ));
}

#[test]
fn errors_serialize_as_messages() {
    let err = Error::InvalidInput("nope".into());
    assert_eq!(serde_json::to_string(&err).unwrap(), "\"nope\"");
}

#[test]
fn note_filter_json_shape() {
    let f: NoteFilter = serde_json::from_str(r#"{"kind":"folder","folderId":"abc"}"#).unwrap();
    assert_eq!(
        f,
        NoteFilter::Folder {
            folder_id: "abc".into()
        }
    );
    let f: NoteFilter = serde_json::from_str(r#"{"kind":"trash"}"#).unwrap();
    assert_eq!(f, NoteFilter::Trash);
}

// --------------------------------------------------------- blank-note cleanup

#[test]
fn blank_notes_are_discarded_but_notes_with_content_are_kept() {
    let s = store();
    let blank = s.create_note(None).unwrap();
    let typed = s.create_note(None).unwrap();
    s.update_note_content(&typed.id, &doc(&["keep me"]))
        .unwrap();
    let checklist = s.create_note(None).unwrap();
    s.update_note_content(
        &checklist.id,
        &json!({ "type": "doc", "content": [{ "type": "taskList", "content": [
            { "type": "taskItem", "attrs": { "checked": false }, "content": [{ "type": "paragraph" }] }
        ]}]}),
    )
    .unwrap();

    assert!(s.discard_note_if_blank(&blank.id).unwrap());
    assert!(matches!(s.get_note(&blank.id), Err(Error::NotFound { .. })));
    assert!(!s.discard_note_if_blank(&typed.id).unwrap());
    assert!(!s.discard_note_if_blank(&checklist.id).unwrap());
    assert!(!s.discard_note_if_blank("missing").unwrap());

    // Blank notes in the trash are left alone (they're already on their way out).
    let trashed = s.create_note(None).unwrap();
    s.trash_note(&trashed.id).unwrap();
    assert!(!s.discard_note_if_blank(&trashed.id).unwrap());

    // Startup cleanup.
    s.create_note(None).unwrap();
    s.create_note(None).unwrap();
    assert_eq!(s.discard_blank_notes().unwrap(), 2);
    assert_eq!(s.list_notes(&NoteFilter::All).unwrap().len(), 2);
}
