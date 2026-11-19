//! Password-encrypted backup files.
//!
//! A backup archive (see [`crate::backup`]) can be encrypted before it leaves the
//! computer. The key comes from a password via Argon2id; the salt and cost settings are
//! stored in the file's header, so any copy of TuxNotes can open it with the password.
//!
//! The data is encrypted in 1 MiB chunks with XChaCha20-Poly1305 (the "STREAM"
//! construction): each chunk's nonce carries its number and a "last chunk" flag, and every
//! chunk authenticates the whole header. Reordered, removed, appended or cut-off chunks,
//! or a changed header, are all detected.
//!
//! Layout: header (64 bytes) followed by chunks of `chunk_size + 16` bytes. The last one
//! can be shorter; it is empty only when the whole file is.
//!
//! | bytes  | content                                   |
//! |--------|-------------------------------------------|
//! | 0–11   | magic `TuxNotesEnc\0`                     |
//! | 12     | format version (1)                        |
//! | 13–24  | Argon2id memory KiB, passes, lanes (u32 LE) |
//! | 25–40  | salt                                      |
//! | 41–59  | nonce prefix (random)                     |
//! | 60–63  | chunk size (u32 LE)                       |

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::crypto::{self, KEY_LEN, KdfParams, NONCE_LEN, SALT_LEN, SecretKey, TAG_LEN};
use crate::error::{Error, Result};

pub const MAGIC: &[u8; 12] = b"TuxNotesEnc\0";
const FORMAT_VERSION: u8 = 1;
pub const HEADER_LEN: usize = 64;
const PREFIX_LEN: usize = NONCE_LEN - 5;
const CHUNK_SIZE: u32 = 1 << 20;
/// Backups can be attacked offline, so ask for a longer password than for locked notes.
pub const MIN_BACKUP_PASSWORD_CHARS: usize = 8;

/// The encryption key for backups, with the salt and settings it was derived with.
pub struct BackupKey {
    pub params: KdfParams,
    pub salt: [u8; SALT_LEN],
    key: SecretKey,
}

impl BackupKey {
    /// A key for a new backup password (random salt, current default costs).
    pub fn create(password: &str) -> Result<Self> {
        Self::create_with(password, KdfParams::default())
    }

    #[doc(hidden)]
    pub fn create_with(password: &str, params: KdfParams) -> Result<Self> {
        if password.chars().count() < MIN_BACKUP_PASSWORD_CHARS {
            return Err(Error::invalid(format!(
                "Use a backup password of at least {MIN_BACKUP_PASSWORD_CHARS} characters."
            )));
        }
        Self::derive(password, params, crypto::random_bytes()?)
    }

    /// The key for `password` with a given salt and settings (e.g. from a file header).
    pub fn derive(password: &str, params: KdfParams, salt: [u8; SALT_LEN]) -> Result<Self> {
        let key = crypto::derive_key(password, &salt, params)?;
        Ok(Self { params, salt, key })
    }

    /// Rebuild a key saved earlier with [`BackupKey::key_bytes`].
    pub fn from_parts(params: KdfParams, salt: [u8; SALT_LEN], key: &[u8]) -> Result<Self> {
        params.validate()?;
        let key: [u8; KEY_LEN] = key
            .try_into()
            .map_err(|_| Error::Crypto("damaged backup key".into()))?;
        Ok(Self {
            params,
            salt,
            key: SecretKey::new(key),
        })
    }

    pub fn key_bytes(&self) -> &[u8; KEY_LEN] {
        &self.key
    }

    /// Whether this key was made for the same password setup as a file's header.
    pub fn fits(&self, header: &Header) -> bool {
        self.salt == header.salt && self.params == header.params
    }
}

/// The readable part of an encrypted backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub params: KdfParams,
    pub salt: [u8; SALT_LEN],
    prefix: [u8; PREFIX_LEN],
    chunk_size: u32,
    raw: [u8; HEADER_LEN],
}

impl Header {
    fn new(key: &BackupKey) -> Result<Self> {
        let prefix: [u8; PREFIX_LEN] = crypto::random_bytes()?;
        let mut raw = [0u8; HEADER_LEN];
        raw[..12].copy_from_slice(MAGIC);
        raw[12] = FORMAT_VERSION;
        raw[13..17].copy_from_slice(&key.params.m_kib.to_le_bytes());
        raw[17..21].copy_from_slice(&key.params.t.to_le_bytes());
        raw[21..25].copy_from_slice(&key.params.p.to_le_bytes());
        raw[25..41].copy_from_slice(&key.salt);
        raw[41..60].copy_from_slice(&prefix);
        raw[60..64].copy_from_slice(&CHUNK_SIZE.to_le_bytes());
        Self::parse(&raw)
    }

    fn parse(raw: &[u8; HEADER_LEN]) -> Result<Self> {
        if &raw[..12] != MAGIC {
            return Err(Error::InvalidBackup(
                "Not an encrypted TuxNotes backup.".into(),
            ));
        }
        if raw[12] != FORMAT_VERSION {
            return Err(Error::InvalidBackup(
                "This encrypted backup was made by a newer version of TuxNotes.".into(),
            ));
        }
        let u32_at = |i: usize| u32::from_le_bytes(raw[i..i + 4].try_into().unwrap());
        let params = KdfParams {
            m_kib: u32_at(13),
            t: u32_at(17),
            p: u32_at(21),
        };
        params
            .validate()
            .map_err(|_| Error::InvalidBackup("The encrypted backup is damaged.".into()))?;
        let chunk_size = u32_at(60);
        if !(4096..=16 << 20).contains(&chunk_size) {
            return Err(Error::InvalidBackup(
                "The encrypted backup is damaged.".into(),
            ));
        }
        Ok(Self {
            params,
            salt: raw[25..41].try_into().unwrap(),
            prefix: raw[41..60].try_into().unwrap(),
            chunk_size,
            raw: *raw,
        })
    }

    fn nonce(&self, index: u32, last: bool) -> XNonce {
        let mut n = [0u8; NONCE_LEN];
        n[..PREFIX_LEN].copy_from_slice(&self.prefix);
        n[PREFIX_LEN..PREFIX_LEN + 4].copy_from_slice(&index.to_be_bytes());
        n[NONCE_LEN - 1] = u8::from(last);
        XNonce::from(n)
    }
}

/// The header of an encrypted backup, or `None` if `path` isn't one (e.g. a plain zip).
pub fn read_header(path: &Path) -> Result<Option<Header>> {
    let mut raw = [0u8; HEADER_LEN];
    let mut f = File::open(path)?;
    let n = read_full(&mut f, &mut raw)?;
    if n < MAGIC.len() || &raw[..MAGIC.len()] != MAGIC {
        return Ok(None);
    }
    if n < HEADER_LEN {
        return Err(Error::InvalidBackup(
            "The encrypted backup is damaged.".into(),
        ));
    }
    Header::parse(&raw).map(Some)
}

/// Encrypt the file at `src` into `dest`.
pub fn encrypt_file(src: &Path, dest: &Path, key: &BackupKey) -> Result<()> {
    let header = Header::new(key)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.key_bytes())
        .map_err(|e| Error::Crypto(e.to_string()))?;
    let result = (|| {
        let mut input = BufReader::new(File::open(src)?);
        let mut out = BufWriter::new(File::create(dest)?);
        out.write_all(&header.raw)?;
        let size = header.chunk_size as usize;
        let mut current = vec![0u8; size];
        let mut next = vec![0u8; size];
        let mut len = read_full(&mut input, &mut current)?;
        let mut index: u32 = 0;
        loop {
            // Look ahead: the chunk is the last one when nothing follows it.
            let next_len = if len == size {
                read_full(&mut input, &mut next)?
            } else {
                0
            };
            let last = next_len == 0;
            let sealed = cipher
                .encrypt(
                    &header.nonce(index, last),
                    Payload {
                        msg: &current[..len],
                        aad: &header.raw,
                    },
                )
                .map_err(|_| Error::Crypto("encryption failed".into()))?;
            out.write_all(&sealed)?;
            if last {
                break;
            }
            std::mem::swap(&mut current, &mut next);
            len = next_len;
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::invalid("This backup is too large to encrypt."))?;
        }
        out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}

/// Decrypt the encrypted backup at `src` into `dest`. A wrong password is reported as
/// [`Error::WrongPassword`]; anything else wrong with the file as [`Error::InvalidBackup`].
pub fn decrypt_file(src: &Path, dest: &Path, key: &BackupKey) -> Result<()> {
    let result = (|| {
        let mut input = BufReader::new(File::open(src)?);
        let mut raw = [0u8; HEADER_LEN];
        if read_full(&mut input, &mut raw)? < HEADER_LEN {
            return Err(Error::InvalidBackup(
                "The encrypted backup is damaged.".into(),
            ));
        }
        let header = Header::parse(&raw)?;
        if !key.fits(&header) {
            return Err(Error::WrongPassword);
        }
        let cipher = XChaCha20Poly1305::new_from_slice(key.key_bytes())
            .map_err(|e| Error::Crypto(e.to_string()))?;
        let mut out = BufWriter::new(File::create(dest)?);
        let size = header.chunk_size as usize + TAG_LEN;
        let mut current = vec![0u8; size];
        let mut next = vec![0u8; size];
        let mut len = read_full(&mut input, &mut current)?;
        let mut index: u32 = 0;
        loop {
            let next_len = if len == size {
                read_full(&mut input, &mut next)?
            } else {
                0
            };
            let last = next_len == 0;
            let plain = cipher
                .decrypt(
                    &header.nonce(index, last),
                    Payload {
                        msg: &current[..len],
                        aad: &header.raw,
                    },
                )
                .map_err(|_| {
                    if index == 0 {
                        // The first chunk is where a wrong password shows up.
                        Error::WrongPassword
                    } else {
                        Error::InvalidBackup(
                            "The encrypted backup is damaged or incomplete.".into(),
                        )
                    }
                })?;
            out.write_all(&plain)?;
            if last {
                break;
            }
            std::mem::swap(&mut current, &mut next);
            len = next_len;
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::InvalidBackup("The encrypted backup is damaged.".into()))?;
        }
        out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}

/// Read until `buf` is full or the input ends; returns how many bytes were read.
fn read_full(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}
