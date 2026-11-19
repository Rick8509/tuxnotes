use serde::{Serialize, Serializer};

/// All errors produced by the TuxNotes core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("invalid note content: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{kind} not found: {id}")]
    NotFound { kind: &'static str, id: String },

    #[error("{0}")]
    InvalidInput(String),

    #[error("file error: {0}")]
    Io(#[from] std::io::Error),

    #[error("backup archive error: {0}")]
    Zip(#[from] zip::result::ZipError),

    /// The backup file is unusable (corrupt, tampered with, or from a newer version).
    #[error("{0}")]
    InvalidBackup(String),

    #[error("Wrong password.")]
    WrongPassword,

    /// Encryption or decryption failed (wrong key, damaged data, no randomness).
    #[error("encryption error: {0}")]
    Crypto(String),

    #[error(
        "this database was created by a newer version of TuxNotes \
         (schema {found}, this version supports up to {supported})"
    )]
    UnsupportedSchema { found: i64, supported: i64 },
}

impl Error {
    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        Error::InvalidInput(msg.into())
    }

    pub(crate) fn not_found(kind: &'static str, id: impl Into<String>) -> Self {
        Error::NotFound {
            kind,
            id: id.into(),
        }
    }
}

/// Errors cross the IPC boundary as plain strings.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
