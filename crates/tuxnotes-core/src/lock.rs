//! Password-locked notes.
//!
//! One password protects all locked notes (like Apple Notes). How it works:
//!
//! - A random 256-bit **notes key** encrypts each locked note's document
//!   (XChaCha20-Poly1305, bound to the note's id so blobs can't be swapped between notes).
//! - The notes key is stored only in encrypted ("wrapped") form, under a key derived from
//!   the password with Argon2id. Changing the password re-wraps it; notes stay as they are.
//! - While locked notes are unlocked, the notes key lives in memory only (wiped on lock,
//!   and gone when TuxNotes quits). It is never written to disk unencrypted.
//! - A locked note's title stays visible (it's the note's first line); its body, preview
//!   and search text are removed. Images and files it uses are not encrypted.
//!
//! There is no way to recover a forgotten password; that's what makes it secure.

use std::collections::{BTreeSet, HashSet};

use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;
use zeroize::Zeroizing;

use crate::attachments::referenced_ids;
use crate::crypto::{self, KEY_LEN, KdfParams, SALT_LEN, SecretKey};
use crate::error::{Error, Result};
use crate::models::NoteSummary;
use crate::store::Store;
use crate::text;

/// Authenticated context for the wrapped notes key.
const WRAP_AAD: &[u8] = b"tuxnotes-notes-key-v1";
pub const MIN_PASSWORD_CHARS: usize = 4;
pub const MAX_PASSWORD_CHARS: usize = 1024;
pub const MAX_HINT_CHARS: usize = 200;
/// What a locked note's visible document is replaced with.
const HIDDEN_DOC: &str = r#"{"type":"doc","content":[{"type":"paragraph"}]}"#;

/// Whether locked notes are set up and currently unlocked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockStatus {
    /// A password for locked notes has been set.
    pub password_set: bool,
    /// Locked notes can be read and edited right now.
    pub unlocked: bool,
    pub hint: String,
    /// How many notes are locked (including ones in Recently Deleted).
    pub locked_notes: i64,
}

struct KeyRecord {
    params: KdfParams,
    salt: Vec<u8>,
    wrapped: Vec<u8>,
    hint: String,
}

impl Store {
    pub fn lock_status(&self) -> Result<LockStatus> {
        let record = self.key_record()?;
        Ok(LockStatus {
            password_set: record.is_some(),
            unlocked: self.notes_key.is_some(),
            hint: record.map(|r| r.hint).unwrap_or_default(),
            locked_notes: self.conn.query_row(
                "SELECT COUNT(*) FROM notes WHERE locked_blob IS NOT NULL",
                [],
                |row| row.get(0),
            )?,
        })
    }

    /// Set the password for locked notes (once; use [`Store::change_lock_password`] after).
    /// Leaves locked notes unlocked.
    pub fn set_lock_password(&mut self, password: &str, hint: &str) -> Result<()> {
        self.set_lock_password_with(password, hint, KdfParams::default())
    }

    #[doc(hidden)]
    /// Like [`Store::set_lock_password`] with explicit key-derivation costs (fast tests).
    pub fn set_lock_password_with(
        &mut self,
        password: &str,
        hint: &str,
        params: KdfParams,
    ) -> Result<()> {
        if self.key_record()?.is_some() {
            return Err(Error::invalid(
                "A password for locked notes is already set.",
            ));
        }
        let hint = check_password_and_hint(password, hint)?;
        let notes_key = crypto::random_key()?;
        let (salt, wrapped) = wrap(&notes_key, password, params)?;
        let now = self.now();
        self.conn.execute(
            "INSERT INTO lock_key (id, kdf, m_kib, t_cost, p_cost, salt, wrapped_key, hint, created_at, updated_at)
             VALUES (1, 'argon2id', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![params.m_kib, params.t, params.p, salt, wrapped, hint, now],
        )?;
        self.notes_key = Some(notes_key);
        Ok(())
    }

    /// Unlock locked notes. Fails with [`Error::WrongPassword`].
    pub fn unlock_notes(&mut self, password: &str) -> Result<()> {
        let record = self
            .key_record()?
            .ok_or_else(|| Error::invalid("No password has been set for locked notes."))?;
        self.notes_key = Some(unwrap(&record, password)?);
        Ok(())
    }

    /// Lock all locked notes again (forget the key).
    pub fn lock_notes(&mut self) {
        self.notes_key = None; // `Zeroizing` wipes it
    }

    /// Change the password. Notes stay encrypted with the same notes key, which is
    /// re-wrapped under the new password (with current key-derivation costs).
    pub fn change_lock_password(&mut self, old: &str, new: &str, hint: &str) -> Result<()> {
        self.change_lock_password_with(old, new, hint, KdfParams::default())
    }

    #[doc(hidden)]
    pub fn change_lock_password_with(
        &mut self,
        old: &str,
        new: &str,
        hint: &str,
        params: KdfParams,
    ) -> Result<()> {
        let record = self
            .key_record()?
            .ok_or_else(|| Error::invalid("No password has been set for locked notes."))?;
        let notes_key = unwrap(&record, old)?;
        let hint = check_password_and_hint(new, hint)?;
        let (salt, wrapped) = wrap(&notes_key, new, params)?;
        self.conn.execute(
            "UPDATE lock_key SET kdf = 'argon2id', m_kib = ?1, t_cost = ?2, p_cost = ?3,
                 salt = ?4, wrapped_key = ?5, hint = ?6, updated_at = ?7
             WHERE id = 1",
            params![
                params.m_kib,
                params.t,
                params.p,
                salt,
                wrapped,
                hint,
                self.now()
            ],
        )?;
        self.notes_key = Some(notes_key);
        Ok(())
    }

    /// Lock a note: encrypt its document. Needs locked notes to be unlocked (or a
    /// password to be set first). Doesn't change the note's modified time.
    pub fn lock_note(&self, id: &str) -> Result<NoteSummary> {
        self.require_active(id)?;
        let key = self.require_notes_key()?;
        if !self.is_locked(id)? {
            let json: String = self.conn.query_row(
                "SELECT content_json FROM notes WHERE id = ?1",
                [id],
                |row| row.get(0),
            )?;
            let doc: Value = serde_json::from_str(&json)?;
            self.write_locked(key, id, &doc, false)?;
            self.scrub();
        }
        self.get_note_summary(id)
    }

    /// Remove a note's lock: store its document unencrypted again.
    pub fn remove_note_lock(&self, id: &str) -> Result<NoteSummary> {
        self.require_active(id)?;
        let Some(blob) = self.locked_blob(id)? else {
            return self.get_note_summary(id);
        };
        let key = self.require_notes_key()?;
        let doc = self.decrypt_note(key, id, &blob)?;
        let extracted = text::extract(&doc);
        self.conn.execute(
            "UPDATE notes
             SET locked_blob = NULL, locked_attachments = '', content_json = ?2,
                 title = ?3, snippet = ?4, content_text = ?5
             WHERE id = ?1",
            params![
                id,
                serde_json::to_string(&doc)?,
                extracted.title,
                extracted.snippet,
                extracted.plain
            ],
        )?;
        self.get_note_summary(id)
    }

    // ------------------------------------------------------------- internals

    pub(crate) fn is_locked(&self, id: &str) -> Result<bool> {
        Ok(self.locked_blob(id)?.is_some())
    }

    fn locked_blob(&self, id: &str) -> Result<Option<Vec<u8>>> {
        self.conn
            .query_row("SELECT locked_blob FROM notes WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or_else(|| Error::not_found("note", id))
    }

    pub(crate) fn require_notes_key(&self) -> Result<&[u8; KEY_LEN]> {
        match &self.notes_key {
            Some(key) => Ok(key),
            None if self.key_record()?.is_none() => {
                Err(Error::invalid("Set a password for locked notes first."))
            }
            None => Err(Error::invalid(
                "This note is locked. Enter the password to open it.",
            )),
        }
    }

    pub(crate) fn decrypt_note(&self, key: &[u8; KEY_LEN], id: &str, blob: &[u8]) -> Result<Value> {
        let json = crypto::open(key, blob, id.as_bytes())
            .map_err(|_| Error::Crypto(format!("locked note {id} can't be decrypted")))?;
        Ok(serde_json::from_slice(&json)?)
    }

    /// Store `doc` encrypted as the note's content. Only the title stays readable.
    pub(crate) fn write_locked(
        &self,
        key: &[u8; KEY_LEN],
        id: &str,
        doc: &Value,
        touch: bool,
    ) -> Result<()> {
        let json = Zeroizing::new(serde_json::to_vec(doc)?);
        let blob = crypto::seal(key, &json, id.as_bytes())?;
        let mut ids = HashSet::new();
        referenced_ids(doc, &mut ids);
        let ids: Vec<String> = ids
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let title = text::extract(doc).title;
        let sql = format!(
            "UPDATE notes
             SET locked_blob = ?2, locked_attachments = ?3, content_json = ?4,
                 title = ?5, snippet = '', content_text = ''{}
             WHERE id = ?1",
            if touch { ", updated_at = ?6" } else { "" }
        );
        if touch {
            self.conn.execute(
                &sql,
                params![id, blob, ids.join(" "), HIDDEN_DOC, title, self.now()],
            )?;
        } else {
            self.conn
                .execute(&sql, params![id, blob, ids.join(" "), HIDDEN_DOC, title])?;
        }
        Ok(())
    }

    /// Best effort: drop the plain text a newly locked note leaves behind in the search
    /// index and the write-ahead log (deleted pages are zeroed by `secure_delete`).
    fn scrub(&self) {
        let _ = self
            .conn
            .execute("INSERT INTO notes_fts(notes_fts) VALUES ('optimize')", []);
        let _ = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    }

    fn key_record(&self) -> Result<Option<KeyRecord>> {
        Ok(self
            .conn
            .query_row(
                "SELECT m_kib, t_cost, p_cost, salt, wrapped_key, hint FROM lock_key WHERE id = 1",
                [],
                |row| {
                    Ok(KeyRecord {
                        params: KdfParams {
                            m_kib: row.get(0)?,
                            t: row.get(1)?,
                            p: row.get(2)?,
                        },
                        salt: row.get(3)?,
                        wrapped: row.get(4)?,
                        hint: row.get(5)?,
                    })
                },
            )
            .optional()?)
    }
}

fn wrap(notes_key: &SecretKey, password: &str, params: KdfParams) -> Result<(Vec<u8>, Vec<u8>)> {
    let salt = crypto::random_bytes::<SALT_LEN>()?;
    let kek = crypto::derive_key(password, &salt, params)?;
    let wrapped = crypto::seal(&kek, &notes_key[..], WRAP_AAD)?;
    Ok((salt.to_vec(), wrapped))
}

fn unwrap(record: &KeyRecord, password: &str) -> Result<SecretKey> {
    let kek = crypto::derive_key(password, &record.salt, record.params)?;
    let raw = crypto::open(&kek, &record.wrapped, WRAP_AAD).map_err(|_| Error::WrongPassword)?;
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    if raw.len() != KEY_LEN {
        return Err(Error::Crypto("damaged notes key".into()));
    }
    key.copy_from_slice(&raw);
    Ok(key)
}

fn check_password_and_hint(password: &str, hint: &str) -> Result<String> {
    let chars = password.chars().count();
    if chars < MIN_PASSWORD_CHARS {
        return Err(Error::invalid(format!(
            "Use a password of at least {MIN_PASSWORD_CHARS} characters."
        )));
    }
    if chars > MAX_PASSWORD_CHARS {
        return Err(Error::invalid("That password is too long."));
    }
    let hint = hint.trim();
    if hint.chars().count() > MAX_HINT_CHARS {
        return Err(Error::invalid(format!(
            "The hint can't be longer than {MAX_HINT_CHARS} characters."
        )));
    }
    if !hint.is_empty() && hint.to_lowercase().contains(&password.to_lowercase()) {
        return Err(Error::invalid("The hint can't contain the password."));
    }
    Ok(hint.to_string())
}
