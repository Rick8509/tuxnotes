//! OAuth 2.0 for installed apps: authorization code flow with PKCE and a loopback
//! redirect, as recommended by Google for desktop applications.
//! <https://developers.google.com/identity/protocols/oauth2/native-app>

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::error::{Error, Result};

pub const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
/// Access only to files this app created. Non-sensitive scope.
pub const DRIVE_FILE_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";

/// Refresh access tokens this long before they actually expire.
const EXPIRY_MARGIN_MS: i64 = 60_000;

#[derive(Debug, Clone)]
pub struct OAuthClient {
    pub client_id: String,
    /// Google issues a (non-confidential) secret for Desktop clients and requires it.
    pub client_secret: Option<String>,
    pub auth_url: String,
    pub token_url: String,
    pub revoke_url: String,
}

impl OAuthClient {
    pub fn google(client_id: impl Into<String>, client_secret: Option<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret,
            auth_url: GOOGLE_AUTH_URL.into(),
            token_url: GOOGLE_TOKEN_URL.into(),
            revoke_url: GOOGLE_REVOKE_URL.into(),
        }
    }

    /// The URL to open in the user's browser.
    pub fn authorization_url(&self, redirect_uri: &str, pkce: &Pkce, state: &str) -> String {
        Url::parse_with_params(
            &self.auth_url,
            &[
                ("client_id", self.client_id.as_str()),
                ("redirect_uri", redirect_uri),
                ("response_type", "code"),
                ("scope", DRIVE_FILE_SCOPE),
                ("code_challenge", pkce.challenge.as_str()),
                ("code_challenge_method", "S256"),
                ("state", state),
                // Ask for a refresh token so backups keep working without signing in again.
                ("access_type", "offline"),
                ("prompt", "consent"),
            ],
        )
        .expect("auth_url is a valid URL")
        .into()
    }

    pub async fn exchange_code(
        &self,
        http: &reqwest::Client,
        code: &str,
        pkce: &Pkce,
        redirect_uri: &str,
    ) -> Result<Tokens> {
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", pkce.verifier.as_str()),
            ("redirect_uri", redirect_uri),
            ("client_id", self.client_id.as_str()),
        ];
        if let Some(secret) = &self.client_secret {
            form.push(("client_secret", secret));
        }
        let tokens = self.token_request(http, &form).await?;
        if tokens.refresh_token.is_none() {
            return Err(Error::OAuth(
                "Google didn't return a refresh token. Remove TuxNotes from your Google account's \
                 third-party access page and sign in again."
                    .into(),
            ));
        }
        Ok(tokens)
    }

    /// Get a new access token. The returned `refresh_token` is the one passed in
    /// unless Google rotated it.
    pub async fn refresh(&self, http: &reqwest::Client, refresh_token: &str) -> Result<Tokens> {
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", self.client_id.as_str()),
        ];
        if let Some(secret) = &self.client_secret {
            form.push(("client_secret", secret));
        }
        let mut tokens = self.token_request(http, &form).await?;
        tokens
            .refresh_token
            .get_or_insert_with(|| refresh_token.to_string());
        Ok(tokens)
    }

    /// Revoke a token (refresh or access). Already-invalid tokens count as success.
    pub async fn revoke(&self, http: &reqwest::Client, token: &str) -> Result<()> {
        let resp = http
            .post(&self.revoke_url)
            .form(&[("token", token)])
            .send()
            .await?;
        if resp.status().is_success() || resp.status().as_u16() == 400 {
            Ok(())
        } else {
            Err(Error::Api {
                status: resp.status().as_u16(),
                message: "Couldn't revoke the Google sign-in.".into(),
            })
        }
    }

    async fn token_request(&self, http: &reqwest::Client, form: &[(&str, &str)]) -> Result<Tokens> {
        let resp = http.post(&self.token_url).form(form).send().await?;
        let status = resp.status();
        if status.is_success() {
            let body: TokenResponse = resp.json().await?;
            return Ok(Tokens {
                access_token: body.access_token,
                expires_at: now_ms() + body.expires_in.max(0) * 1000,
                refresh_token: body.refresh_token,
            });
        }
        let body: OAuthErrorBody = resp.json().await.unwrap_or_default();
        match body.error.as_str() {
            // Refresh token revoked/expired, or the auth code was already used.
            "invalid_grant" => Err(Error::SessionExpired),
            "" => Err(Error::Api {
                status: status.as_u16(),
                message: "Unexpected response from Google's sign-in service.".into(),
            }),
            other => Err(Error::OAuth(match body.error_description {
                Some(desc) => format!("{other}: {desc}"),
                None => other.to_string(),
            })),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    /// Unix milliseconds.
    pub expires_at: i64,
    pub refresh_token: Option<String>,
}

impl Tokens {
    pub fn needs_refresh(&self) -> bool {
        now_ms() + EXPIRY_MARGIN_MS >= self.expires_at
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

#[derive(Deserialize, Default)]
struct OAuthErrorBody {
    #[serde(default)]
    error: String,
    error_description: Option<String>,
}

/// PKCE verifier/challenge pair (RFC 7636, S256).
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Self {
        let verifier = random_url_safe(64); // 86 chars, within the 43..=128 limit
        Self {
            challenge: Self::challenge_for(&verifier),
            verifier,
        }
    }

    pub fn challenge_for(verifier: &str) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    }
}

/// An unguessable value for the OAuth `state` parameter.
pub fn random_state() -> String {
    random_url_safe(32)
}

fn random_url_safe(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("the OS random number generator is available");
    URL_SAFE_NO_PAD.encode(buf)
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
