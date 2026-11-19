//! Where TuxNotes gets its Google OAuth client ("Desktop app" client from Google Cloud).
//!
//! Users never see or enter this. In order of precedence:
//! 1. `TUXNOTES_GOOGLE_CLIENT_ID` / `TUXNOTES_GOOGLE_CLIENT_SECRET` environment variables
//!    at runtime (handy for development).
//! 2. `google-client.json` in the TuxNotes config directory — the JSON file Google Cloud
//!    lets you download for the client, used as-is (for people who build TuxNotes themselves).
//! 3. The client compiled into release builds from CI secrets.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use tuxnotes_gdrive::OAuthClient;

pub const CLIENT_FILE_NAME: &str = "google-client.json";

const BUILT_IN_CLIENT_ID: Option<&str> = option_env!("TUXNOTES_GOOGLE_CLIENT_ID");
const BUILT_IN_CLIENT_SECRET: Option<&str> = option_env!("TUXNOTES_GOOGLE_CLIENT_SECRET");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClientSource {
    Environment,
    ConfigFile,
    BuiltIn,
    None,
}

fn non_empty(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|v| !v.is_empty()).map(String::from)
}

/// Resolve the OAuth client, looking for the config file in `config_dir`.
pub fn resolve(config_dir: &Path) -> (ClientSource, Option<OAuthClient>) {
    let env_id = std::env::var("TUXNOTES_GOOGLE_CLIENT_ID").ok();
    if let Some(id) = non_empty(env_id.as_deref()) {
        let secret = std::env::var("TUXNOTES_GOOGLE_CLIENT_SECRET").ok();
        return (
            ClientSource::Environment,
            Some(OAuthClient::google(id, non_empty(secret.as_deref()))),
        );
    }
    if let Ok(raw) = std::fs::read_to_string(config_dir.join(CLIENT_FILE_NAME)) {
        match parse_client_json(&raw) {
            Some((id, secret)) => {
                return (
                    ClientSource::ConfigFile,
                    Some(OAuthClient::google(id, secret)),
                );
            }
            None => log::warn!("{CLIENT_FILE_NAME} has no client_id; ignoring it"),
        }
    }
    if let Some(id) = non_empty(BUILT_IN_CLIENT_ID) {
        return (
            ClientSource::BuiltIn,
            Some(OAuthClient::google(id, non_empty(BUILT_IN_CLIENT_SECRET))),
        );
    }
    (ClientSource::None, None)
}

/// Read a client from the JSON Google Cloud provides for download
/// (`{"installed": {"client_id": …, "client_secret": …}}`), or a flat
/// `{"client_id": …, "client_secret": …}` object.
pub fn parse_client_json(raw: &str) -> Option<(String, Option<String>)> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let obj = value.get("installed").unwrap_or(&value);
    let id = non_empty(obj.get("client_id")?.as_str())?;
    let secret = non_empty(obj.get("client_secret").and_then(Value::as_str));
    Some((id, secret))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_file_google_cloud_downloads() {
        let downloaded = r#"{"installed":{"client_id":"123-abc.apps.googleusercontent.com",
            "project_id":"tuxnotes","auth_uri":"https://accounts.google.com/o/oauth2/auth",
            "token_uri":"https://oauth2.googleapis.com/token","client_secret":"GOCSPX-xyz",
            "redirect_uris":["http://localhost"]}}"#;
        assert_eq!(
            parse_client_json(downloaded),
            Some((
                "123-abc.apps.googleusercontent.com".into(),
                Some("GOCSPX-xyz".into())
            ))
        );
    }

    #[test]
    fn parses_flat_json_and_rejects_garbage() {
        assert_eq!(
            parse_client_json(r#"{"client_id":" id-1 "}"#),
            Some(("id-1".into(), None))
        );
        assert_eq!(parse_client_json(r#"{"client_id":""}"#), None);
        assert_eq!(parse_client_json(r#"{"web":{"client_id":"x"}}"#), None);
        assert_eq!(parse_client_json("not json"), None);
    }

    #[test]
    fn config_file_is_used_when_present() {
        // Only meaningful when the env override isn't set in the test environment.
        if std::env::var("TUXNOTES_GOOGLE_CLIENT_ID").is_ok() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("tuxnotes-client-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(CLIENT_FILE_NAME),
            r#"{"installed":{"client_id":"file-id","client_secret":"s"}}"#,
        )
        .unwrap();
        let (source, client) = resolve(&dir);
        assert_eq!(source, ClientSource::ConfigFile);
        assert_eq!(client.unwrap().client_id, "file-id");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
