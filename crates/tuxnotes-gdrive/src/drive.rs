//! The small subset of the Google Drive v3 API that backups need.

use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::{RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Deserializer};
use serde_json::json;

use crate::error::{Error, Result};

pub const DRIVE_API: &str = "https://www.googleapis.com/drive/v3";
pub const DRIVE_UPLOAD_API: &str = "https://www.googleapis.com/upload/drive/v3";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const FILE_FIELDS: &str = "id,name,size,createdTime,appProperties";
const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    /// Drive returns sizes as strings; absent for folders.
    #[serde(default, deserialize_with = "u64_from_str")]
    pub size: Option<u64>,
    #[serde(default)]
    pub created_time: Option<String>,
    #[serde(default)]
    pub app_properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveUser {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub email_address: String,
}

#[derive(Debug, Clone)]
pub struct DriveClient {
    http: reqwest::Client,
    api: String,
    upload_api: String,
    retry_delay: Duration,
}

impl DriveClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self::with_base_urls(http, DRIVE_API, DRIVE_UPLOAD_API)
    }

    /// Point at a different server (used by tests).
    pub fn with_base_urls(http: reqwest::Client, api: &str, upload_api: &str) -> Self {
        Self {
            http,
            api: api.trim_end_matches('/').to_string(),
            upload_api: upload_api.trim_end_matches('/').to_string(),
            retry_delay: Duration::from_secs(1),
        }
    }

    /// Base delay for exponential backoff between retries (tests use zero).
    pub fn with_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_delay = delay;
        self
    }

    pub async fn about(&self, token: &str) -> Result<DriveUser> {
        #[derive(Deserialize)]
        struct About {
            user: DriveUser,
        }
        let req = self
            .http
            .get(format!("{}/about", self.api))
            .bearer_auth(token)
            .query(&[("fields", "user(displayName,emailAddress)")]);
        let about: About = self.send(req).await?.json().await?;
        Ok(about.user)
    }

    /// Find a top-level folder created by this app, or create it.
    pub async fn find_or_create_folder(&self, token: &str, name: &str) -> Result<String> {
        let q = format!(
            "name = '{}' and mimeType = '{FOLDER_MIME}' and 'root' in parents and trashed = false",
            escape_query(name)
        );
        let existing = self.query_files(token, &q, "createdTime").await?;
        if let Some(folder) = existing.into_iter().next() {
            return Ok(folder.id);
        }
        let req = self
            .http
            .post(format!("{}/files", self.api))
            .bearer_auth(token)
            .query(&[("fields", "id")])
            .json(&json!({ "name": name, "mimeType": FOLDER_MIME, "parents": ["root"] }));
        let created: DriveFile = self.send(req).await?.json().await?;
        Ok(created.id)
    }

    /// Files in `folder_id` carrying app property `key = value`, newest first.
    pub async fn list_files(
        &self,
        token: &str,
        folder_id: &str,
        key: &str,
        value: &str,
    ) -> Result<Vec<DriveFile>> {
        let q = format!(
            "'{}' in parents and trashed = false and appProperties has {{ key='{}' and value='{}' }}",
            escape_query(folder_id),
            escape_query(key),
            escape_query(value)
        );
        self.query_files(token, &q, "createdTime desc").await
    }

    async fn query_files(&self, token: &str, q: &str, order_by: &str) -> Result<Vec<DriveFile>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Page {
            #[serde(default)]
            files: Vec<DriveFile>,
            next_page_token: Option<String>,
        }
        let fields = format!("nextPageToken,files({FILE_FIELDS})");
        let mut files = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut params = vec![
                ("q", q),
                ("orderBy", order_by),
                ("fields", fields.as_str()),
                ("pageSize", "100"),
                ("spaces", "drive"),
            ];
            if let Some(t) = &page_token {
                params.push(("pageToken", t));
            }
            let req = self
                .http
                .get(format!("{}/files", self.api))
                .bearer_auth(token)
                .query(&params);
            let page: Page = self.send(req).await?.json().await?;
            files.extend(page.files);
            match page.next_page_token {
                Some(t) if !t.is_empty() => page_token = Some(t),
                _ => return Ok(files),
            }
        }
    }

    /// Upload a file with a resumable session. Transient failures restart the upload.
    pub async fn upload(
        &self,
        token: &str,
        folder_id: &str,
        name: &str,
        mime_type: &str,
        bytes: Vec<u8>,
        app_properties: &BTreeMap<String, String>,
    ) -> Result<DriveFile> {
        let mut attempt = 1;
        loop {
            match self
                .upload_once(token, folder_id, name, mime_type, &bytes, app_properties)
                .await
            {
                Err(e) if e.is_transient() && attempt < MAX_ATTEMPTS => {
                    tokio::time::sleep(self.backoff(attempt)).await;
                    attempt += 1;
                }
                other => return other,
            }
        }
    }

    async fn upload_once(
        &self,
        token: &str,
        folder_id: &str,
        name: &str,
        mime_type: &str,
        bytes: &[u8],
        app_properties: &BTreeMap<String, String>,
    ) -> Result<DriveFile> {
        let init = self
            .http
            .post(format!("{}/files", self.upload_api))
            .bearer_auth(token)
            .query(&[("uploadType", "resumable"), ("fields", FILE_FIELDS)])
            .header("X-Upload-Content-Type", mime_type)
            .header("X-Upload-Content-Length", bytes.len())
            .json(&json!({
                "name": name,
                "mimeType": mime_type,
                "parents": [folder_id],
                "appProperties": app_properties,
            }));
        let resp = check(init.send().await?).await?;
        let session = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::Api {
                status: resp.status().as_u16(),
                message: "Google Drive didn't start the upload.".into(),
            })?
            .to_string();

        let put = self
            .http
            .put(session)
            .header(reqwest::header::CONTENT_TYPE, mime_type)
            .body(bytes.to_vec());
        Ok(check(put.send().await?).await?.json().await?)
    }

    /// Download a file's contents, refusing anything larger than `max_bytes`.
    pub async fn download(&self, token: &str, file_id: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let req = self
            .http
            .get(format!("{}/files/{}", self.api, encode_path(file_id)))
            .bearer_auth(token)
            .query(&[("alt", "media")]);
        let mut resp = self.send(req).await?;
        if resp.content_length().is_some_and(|len| len > max_bytes) {
            return Err(Error::TooLarge);
        }
        let mut out = Vec::new();
        while let Some(chunk) = resp.chunk().await? {
            if out.len() as u64 + chunk.len() as u64 > max_bytes {
                return Err(Error::TooLarge);
            }
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    /// Permanently delete a file (only works for files this app created).
    pub async fn delete(&self, token: &str, file_id: &str) -> Result<()> {
        let req = self
            .http
            .delete(format!("{}/files/{}", self.api, encode_path(file_id)))
            .bearer_auth(token);
        match self.send(req).await {
            Ok(_) | Err(Error::Api { status: 404, .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Send with retries for transient failures (network errors, 429, 5xx).
    async fn send(&self, req: RequestBuilder) -> Result<Response> {
        let mut attempt = 1;
        loop {
            let this_try = req
                .try_clone()
                .expect("requests without streaming bodies can be cloned");
            let result = match this_try.send().await {
                Ok(resp) => check(resp).await,
                Err(e) => Err(e.into()),
            };
            match result {
                Err(e) if e.is_transient() && attempt < MAX_ATTEMPTS => {
                    tokio::time::sleep(self.backoff(attempt)).await;
                    attempt += 1;
                }
                other => return other,
            }
        }
    }

    fn backoff(&self, attempt: u32) -> Duration {
        self.retry_delay * 2u32.pow(attempt - 1)
    }
}

/// Turn non-success responses into errors with Google's message.
async fn check(resp: Response) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    if status == StatusCode::UNAUTHORIZED {
        return Err(Error::Unauthorized);
    }
    #[derive(Deserialize, Default)]
    struct Body {
        #[serde(default)]
        error: Detail,
    }
    #[derive(Deserialize, Default)]
    struct Detail {
        #[serde(default)]
        message: String,
    }
    let message = resp
        .json::<Body>()
        .await
        .map(|b| b.error.message)
        .unwrap_or_default();
    Err(Error::Api {
        status: status.as_u16(),
        message: if message.is_empty() {
            status
                .canonical_reason()
                .unwrap_or("request failed")
                .to_string()
        } else {
            message
        },
    })
}

/// Escape a value for use inside single quotes in a Drive `q` expression.
fn escape_query(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn encode_path(segment: &str) -> String {
    url::form_urlencoded::byte_serialize(segment.as_bytes()).collect()
}

fn u64_from_str<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<u64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Num {
        S(String),
        N(u64),
    }
    Ok(match Option::<Num>::deserialize(d)? {
        Some(Num::S(s)) => s.parse().ok(),
        Some(Num::N(n)) => Some(n),
        None => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_escaped() {
        assert_eq!(escape_query(r"it's a \ test"), r"it\'s a \\ test");
    }
}
