use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Couldn't reach Google. Check your internet connection and try again. ({0})")]
    Network(reqwest::Error),

    #[error("Google Drive error ({status}): {message}")]
    Api { status: u16, message: String },

    /// The access token was rejected; refresh it and retry.
    #[error("Google rejected the access token.")]
    Unauthorized,

    /// The refresh token no longer works (revoked, expired, or password changed).
    #[error("Your Google sign-in has expired. Please sign in again.")]
    SessionExpired,

    #[error("Google sign-in was cancelled ({0}).")]
    AccessDenied(String),

    #[error("Google sign-in failed: {0}")]
    OAuth(String),

    #[error("The file on Google Drive is larger than expected.")]
    TooLarge,

    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Network(e.without_url())
    }
}

impl Error {
    /// Whether retrying the same request may succeed.
    pub fn is_transient(&self) -> bool {
        match self {
            Error::Network(e) => e.is_connect() || e.is_timeout() || e.is_request(),
            Error::Api { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
