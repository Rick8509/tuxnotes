//! Password-based encryption, used for locked notes (and encrypted backups).
//!
//! - Keys are derived from passwords with **Argon2id**, which is deliberately slow and
//!   memory-hungry so that guessing passwords is expensive.
//! - Data is sealed with **XChaCha20-Poly1305**: authenticated encryption, so a wrong key
//!   or any change to the data is detected. Its 24-byte nonces are safe to pick at random.
//! - Keys live in [`Zeroizing`] buffers, which are wiped from memory when dropped; the
//!   crypto libraries are built with their "zeroize" feature, so their internal copies
//!   (cipher keys, Argon2's intermediate hashes) are wiped too.

use argon2::{Algorithm, Argon2, Block, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

pub const KEY_LEN: usize = 32;
pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 24;
/// Poly1305 authentication tag appended to every ciphertext.
pub const TAG_LEN: usize = 16;
/// First byte of everything [`seal`] produces, so the format can change later.
const SEAL_VERSION: u8 = 1;

/// A 256-bit key, wiped from memory when dropped.
pub type SecretKey = Zeroizing<[u8; KEY_LEN]>;

/// Argon2id cost settings. Stored next to the salt, so they can be raised later
/// without breaking existing data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_kib: u32,
    /// Passes over the memory.
    pub t: u32,
    /// Lanes (parallelism).
    pub p: u32,
}

impl Default for KdfParams {
    /// 64 MiB, 3 passes, 1 lane: RFC 9106's recommendation for memory-constrained
    /// use. Takes a fraction of a second on a typical computer.
    fn default() -> Self {
        Self {
            m_kib: 64 * 1024,
            t: 3,
            p: 1,
        }
    }
}

impl KdfParams {
    /// Refuse values a damaged or hostile file could use to hang the app or exhaust memory.
    pub fn validate(&self) -> Result<()> {
        let ok = (8 * 1024..=1024 * 1024).contains(&self.m_kib)
            && (1..=10).contains(&self.t)
            && (1..=8).contains(&self.p);
        if ok {
            Ok(())
        } else {
            Err(Error::Crypto("unsupported key settings".into()))
        }
    }
}

/// Derive a key from a password and a random salt.
pub fn derive_key(password: &str, salt: &[u8], params: KdfParams) -> Result<SecretKey> {
    params.validate()?;
    let params = Params::new(params.m_kib, params.t, params.p, Some(KEY_LEN))
        .map_err(|e| Error::Crypto(e.to_string()))?;
    // Argon2's working memory (tens of MiB, from which the key could be recomputed) is
    // ours, so it's wiped afterwards; the library would free its own without wiping.
    let mut memory = Zeroizing::new(vec![Block::new(); params.block_count()]);
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into_with_memory(password.as_bytes(), salt, &mut key[..], &mut memory[..])
        .map_err(|e| Error::Crypto(e.to_string()))?;
    Ok(key)
}

/// Cryptographically secure random bytes from the operating system.
pub fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|e| Error::Crypto(format!("no secure randomness: {e}")))?;
    Ok(out)
}

/// A new random key.
pub fn random_key() -> Result<SecretKey> {
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    getrandom::fill(&mut key[..])
        .map_err(|e| Error::Crypto(format!("no secure randomness: {e}")))?;
    Ok(key)
}

/// Encrypt `plaintext`. `aad` is authenticated but not stored: the same value must be
/// passed to [`open`] (it binds the ciphertext to its context, e.g. a note id).
///
/// Output: version (1 byte) ‖ random nonce (24) ‖ ciphertext ‖ tag (16).
pub fn seal(key: &[u8; KEY_LEN], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let cipher =
        XChaCha20Poly1305::new_from_slice(key).map_err(|e| Error::Crypto(e.to_string()))?;
    let nonce = random_bytes::<NONCE_LEN>()?;
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| Error::Crypto("encryption failed".into()))?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
    out.push(SEAL_VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt data made by [`seal`]. Fails with [`Error::Crypto`] if the key or `aad` is
/// wrong or the data was changed.
pub fn open(key: &[u8; KEY_LEN], sealed: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if sealed.len() < 1 + NONCE_LEN + TAG_LEN || sealed[0] != SEAL_VERSION {
        return Err(Error::Crypto("unknown or damaged encrypted data".into()));
    }
    let nonce: [u8; NONCE_LEN] = sealed[1..1 + NONCE_LEN].try_into().expect("checked length");
    let cipher =
        XChaCha20Poly1305::new_from_slice(key).map_err(|e| Error::Crypto(e.to_string()))?;
    cipher
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &sealed[1 + NONCE_LEN..],
                aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| Error::Crypto("wrong key or damaged data".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: KdfParams = KdfParams {
        m_kib: 8 * 1024,
        t: 1,
        p: 1,
    };

    #[test]
    fn seal_and_open_round_trip() {
        let key = random_key().unwrap();
        let sealed = seal(&key, b"secret plan", b"note-1").unwrap();
        assert_eq!(sealed.len(), 1 + NONCE_LEN + 11 + TAG_LEN);
        assert!(!sealed.windows(6).any(|w| w == b"secret"));
        assert_eq!(&open(&key, &sealed, b"note-1").unwrap()[..], b"secret plan");
        // Same input, different nonce: never the same ciphertext twice.
        assert_ne!(seal(&key, b"secret plan", b"note-1").unwrap(), sealed);
    }

    #[test]
    fn wrong_key_context_or_tampering_is_detected() {
        let key = random_key().unwrap();
        let sealed = seal(&key, b"secret", b"note-1").unwrap();
        assert!(open(&random_key().unwrap(), &sealed, b"note-1").is_err());
        assert!(
            open(&key, &sealed, b"note-2").is_err(),
            "bound to its context"
        );
        for i in [0, 5, sealed.len() - 1] {
            let mut bad = sealed.clone();
            bad[i] ^= 1;
            assert!(open(&key, &bad, b"note-1").is_err(), "flipped byte {i}");
        }
        assert!(open(&key, &sealed[..10], b"note-1").is_err());
        assert!(open(&key, &[], b"note-1").is_err());
    }

    #[test]
    fn derived_keys_depend_on_password_salt_and_settings() {
        let salt = [7u8; SALT_LEN];
        let k1 = derive_key("correct horse", &salt, FAST).unwrap();
        assert_eq!(k1, derive_key("correct horse", &salt, FAST).unwrap());
        assert_ne!(k1, derive_key("correct horsf", &salt, FAST).unwrap());
        assert_ne!(
            k1,
            derive_key("correct horse", &[8u8; SALT_LEN], FAST).unwrap()
        );
        assert_ne!(
            k1,
            derive_key("correct horse", &salt, KdfParams { t: 2, ..FAST }).unwrap()
        );
    }

    #[test]
    fn argon2id_matches_an_independent_implementation() {
        // Computed with Node.js crypto.argon2Sync (which reproduces the RFC 9106
        // Argon2id test vector): password "password", salt "somesalt16bytes!",
        // 8 MiB, 1 pass, 1 lane, 32-byte output.
        let key = derive_key("password", b"somesalt16bytes!", FAST).unwrap();
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "4fdbbd2bbe29c3e19838692b12f8bd629277c01e89996c01e8247bd41d3c1f1f"
        );
    }

    #[test]
    fn the_libraries_wipe_their_secrets() {
        // Fails to compile if the crypto libraries lose their "zeroize" feature.
        fn wipes_on_drop<T: zeroize::ZeroizeOnDrop>() {}
        wipes_on_drop::<XChaCha20Poly1305>();
        fn wipeable<T: zeroize::Zeroize>() {}
        wipeable::<Block>();
    }

    #[test]
    fn absurd_settings_are_refused() {
        for bad in [
            KdfParams { m_kib: 1, ..FAST },
            KdfParams {
                m_kib: u32::MAX,
                ..FAST
            },
            KdfParams { t: 0, ..FAST },
            KdfParams { t: 1000, ..FAST },
            KdfParams { p: 0, ..FAST },
        ] {
            assert!(derive_key("pw", &[0u8; SALT_LEN], bad).is_err(), "{bad:?}");
        }
        KdfParams::default().validate().unwrap();
    }
}
