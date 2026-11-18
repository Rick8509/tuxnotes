//! Storage for the Google refresh token.
//!
//! Preferred: the desktop keyring via the Secret Service API (GNOME Keyring,
//! KWallet, KeePassXC…). If no keyring is available (e.g. minimal window
//! managers), the token is written to a file readable only by the current user.
//!
//! All methods block (D-Bus round trips); call them from `spawn_blocking`.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

const SERVICE: &str = "io.github.tuxnotes.TuxNotes";
const ACCOUNT: &str = "google-drive-refresh-token";

#[derive(Clone)]
pub struct TokenVault {
    fallback_path: PathBuf,
}

impl TokenVault {
    pub fn new(fallback_path: PathBuf) -> Self {
        Self { fallback_path }
    }

    fn entry() -> keyring::Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, ACCOUNT)
    }

    pub fn load(&self) -> Option<String> {
        match Self::entry().and_then(|e| e.get_password()) {
            Ok(token) => return Some(token),
            Err(keyring::Error::NoEntry) => {}
            Err(e) => log::warn!("keyring unavailable ({e}); checking fallback file"),
        }
        fs::read_to_string(&self.fallback_path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn save(&self, token: &str) -> std::io::Result<()> {
        match Self::entry().and_then(|e| e.set_password(token)) {
            Ok(()) => {
                let _ = fs::remove_file(&self.fallback_path);
                Ok(())
            }
            Err(e) => {
                log::warn!(
                    "keyring unavailable ({e}); storing the Google token in {} (owner-only)",
                    self.fallback_path.display()
                );
                write_private(&self.fallback_path, token)
            }
        }
    }

    pub fn clear(&self) {
        match Self::entry().and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => log::warn!("could not remove token from keyring: {e}"),
        }
        let _ = fs::remove_file(&self.fallback_path);
    }
}

/// Write a file with mode 0600, created that way from the start.
fn write_private(path: &PathBuf, contents: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let _ = fs::remove_file(path);
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(contents.as_bytes())
}
