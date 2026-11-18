//! # tuxnotes-gdrive
//!
//! Google sign-in for desktop apps (loopback redirect + PKCE) and the small part of
//! the Google Drive v3 API that TuxNotes needs to store and fetch backups.
//!
//! Uses the `drive.file` scope, so TuxNotes can only ever see files it created.

pub mod drive;
pub mod error;
pub mod loopback;
pub mod oauth;

pub use drive::{DriveClient, DriveFile, DriveUser};
pub use error::{Error, Result};
pub use loopback::LoopbackReceiver;
pub use oauth::{OAuthClient, Pkce, Tokens, random_state};
