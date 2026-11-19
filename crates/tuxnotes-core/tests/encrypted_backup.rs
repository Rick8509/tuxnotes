use std::fs;
use std::path::Path;

use serde_json::json;
use tuxnotes_core::backup::{create_backup, extract_backup};
use tuxnotes_core::crypto::KdfParams;
use tuxnotes_core::encrypted_backup::{
    BackupKey, HEADER_LEN, decrypt_file, encrypt_file, read_header,
};
use tuxnotes_core::{Error, Store};

const FAST: KdfParams = KdfParams {
    m_kib: 8 * 1024,
    t: 1,
    p: 1,
};
const MIB: usize = 1 << 20;

fn key() -> BackupKey {
    BackupKey::create_with("correct horse battery", FAST).unwrap()
}

/// Pseudo-random bytes (so nothing compresses or repeats by accident).
fn data(len: usize) -> Vec<u8> {
    let mut x: u32 = 0x1234_5678;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn round_trip(dir: &Path, plain: &[u8], key: &BackupKey) -> Vec<u8> {
    let (src, enc, out) = (dir.join("plain"), dir.join("enc"), dir.join("out"));
    fs::write(&src, plain).unwrap();
    encrypt_file(&src, &enc, key).unwrap();
    decrypt_file(&enc, &out, key).unwrap();
    fs::read(out).unwrap()
}

#[test]
fn files_of_any_size_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let k = key();
    for len in [
        0,
        1,
        1000,
        MIB - 1,
        MIB,
        MIB + 1,
        2 * MIB,
        2 * MIB + MIB / 2,
    ] {
        let plain = data(len);
        assert_eq!(round_trip(dir.path(), &plain, &k), plain, "{len} bytes");
        let chunks = len.div_ceil(MIB).max(1); // an empty file still gets one (empty) chunk
        let expected = HEADER_LEN + len + chunks * 16;
        assert_eq!(
            fs::metadata(dir.path().join("enc")).unwrap().len() as usize,
            expected,
            "{len} bytes"
        );
    }
}

#[test]
fn the_password_and_header_settings_are_what_open_it() {
    let dir = tempfile::tempdir().unwrap();
    let k = key();
    let (src, enc, out) = (
        dir.path().join("p"),
        dir.path().join("e"),
        dir.path().join("o"),
    );
    fs::write(&src, b"my notes").unwrap();
    encrypt_file(&src, &enc, &k).unwrap();
    assert!(!fs::read(&enc).unwrap().windows(8).any(|w| w == b"my notes"));

    // Another computer: only the password is known; salt and settings come from the file.
    let header = read_header(&enc).unwrap().expect("encrypted");
    assert!(k.fits(&header));
    let again = BackupKey::derive("correct horse battery", header.params, header.salt).unwrap();
    decrypt_file(&enc, &out, &again).unwrap();
    assert_eq!(fs::read(&out).unwrap(), b"my notes");

    let wrong = BackupKey::derive("correct horse batterY", header.params, header.salt).unwrap();
    assert!(matches!(
        decrypt_file(&enc, &out, &wrong),
        Err(Error::WrongPassword)
    ));
    assert!(!out.exists(), "nothing is left behind after a failure");
    // A key from a different password setup (other salt) is refused up front.
    assert!(matches!(
        decrypt_file(&enc, &out, &key()),
        Err(Error::WrongPassword)
    ));
}

#[test]
fn plain_files_have_no_header() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.zip");
    fs::write(&p, b"PK\x03\x04 not encrypted").unwrap();
    assert!(read_header(&p).unwrap().is_none());
    fs::write(&p, b"").unwrap();
    assert!(read_header(&p).unwrap().is_none());
}

#[test]
fn tampering_truncation_and_reordering_are_detected() {
    let dir = tempfile::tempdir().unwrap();
    let k = key();
    let (src, enc, bad, out) = (
        dir.path().join("p"),
        dir.path().join("e"),
        dir.path().join("b"),
        dir.path().join("o"),
    );
    fs::write(&src, data(2 * MIB + 1000)).unwrap(); // chunks: full, full, partial (last)
    encrypt_file(&src, &enc, &k).unwrap();
    let good = fs::read(&enc).unwrap();
    let chunk = MIB + 16;

    let attempt = |bytes: &[u8]| {
        fs::write(&bad, bytes).unwrap();
        decrypt_file(&bad, &out, &k)
    };
    // Cut off at a chunk boundary (the dropped part was the real last chunk).
    assert!(attempt(&good[..HEADER_LEN + 2 * chunk]).is_err());
    // Cut off in the middle, or a byte missing at the end.
    assert!(attempt(&good[..HEADER_LEN + chunk + 500]).is_err());
    assert!(attempt(&good[..good.len() - 1]).is_err());
    // Extra bytes appended.
    let mut longer = good.clone();
    longer.extend_from_slice(&[0u8; 40]);
    assert!(attempt(&longer).is_err());
    // Two chunks swapped.
    let mut swapped = good[..HEADER_LEN].to_vec();
    swapped.extend_from_slice(&good[HEADER_LEN + chunk..HEADER_LEN + 2 * chunk]);
    swapped.extend_from_slice(&good[HEADER_LEN..HEADER_LEN + chunk]);
    swapped.extend_from_slice(&good[HEADER_LEN + 2 * chunk..]);
    assert!(attempt(&swapped).is_err());
    // A flipped bit in the data, or in the nonce prefix of the header.
    for at in [HEADER_LEN + chunk + 7, 50] {
        let mut flipped = good.clone();
        flipped[at] ^= 1;
        assert!(attempt(&flipped).is_err(), "byte {at}");
    }
    // Absurd key settings in the header are refused rather than run.
    let mut greedy = good.clone();
    greedy[13..17].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(attempt(&greedy), Err(Error::InvalidBackup(_))));
    assert!(attempt(&good).is_ok());
}

#[test]
fn backup_passwords_must_be_long_enough() {
    assert!(matches!(
        BackupKey::create_with("short", FAST),
        Err(Error::InvalidInput(_))
    ));
    let k = BackupKey::create_with("long enough", FAST).unwrap();
    let saved = BackupKey::from_parts(k.params, k.salt, k.key_bytes()).unwrap();
    assert_eq!(saved.key_bytes(), k.key_bytes());
    assert!(BackupKey::from_parts(k.params, k.salt, &[1, 2, 3]).is_err());
}

#[test]
fn a_whole_backup_survives_encryption() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let n = store.create_note(None).unwrap();
    let doc = json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "Secret recipe" }] }] });
    store.update_note_content(&n.id, &doc).unwrap();
    let (zip, enc, back) = (
        dir.path().join("b.zip"),
        dir.path().join("b.zip.enc"),
        dir.path().join("back.zip"),
    );
    create_backup(&store, None, &zip, "test").unwrap();
    let k = key();
    encrypt_file(&zip, &enc, &k).unwrap();
    assert!(
        !fs::read(&enc)
            .unwrap()
            .windows(13)
            .any(|w| w == b"Secret recipe")
    );
    decrypt_file(&enc, &back, &k).unwrap();
    let manifest = extract_backup(&back, &dir.path().join("restored.db"), None).unwrap();
    assert_eq!(manifest.note_count, 1);
    let restored = Store::open(dir.path().join("restored.db")).unwrap();
    assert_eq!(restored.get_note(&n.id).unwrap().content, doc);
}

#[test]
fn change_tracking_sees_edits() {
    let store = Store::open_in_memory().unwrap();
    let before = store.change_count();
    let t0 = store.last_change_at().unwrap();
    let n = store.create_note(None).unwrap();
    assert!(store.change_count() > before);
    assert!(store.last_change_at().unwrap() > t0);
    let mid = store.change_count();
    store.get_note(&n.id).unwrap();
    store.list_notes(&tuxnotes_core::NoteFilter::All).unwrap();
    assert_eq!(store.change_count(), mid, "reading changes nothing");
}
