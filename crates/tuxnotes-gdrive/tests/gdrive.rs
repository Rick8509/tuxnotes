use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::json;
use tuxnotes_gdrive::{DriveClient, Error, LoopbackReceiver, OAuthClient, Pkce};
use wiremock::matchers::{
    body_partial_json, body_string_contains, header, method, path, query_param,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn http() -> reqwest::Client {
    reqwest::Client::new()
}

async fn drive(server: &MockServer) -> DriveClient {
    DriveClient::with_base_urls(
        http(),
        &format!("{}/drive/v3", server.uri()),
        &format!("{}/upload/drive/v3", server.uri()),
    )
    .with_retry_delay(Duration::ZERO)
}

fn oauth(server: &MockServer) -> OAuthClient {
    let mut c = OAuthClient::google("client-123", Some("secret-xyz".into()));
    c.token_url = format!("{}/token", server.uri());
    c.revoke_url = format!("{}/revoke", server.uri());
    c
}

// ------------------------------------------------------------------- PKCE / URLs

#[test]
fn pkce_matches_rfc7636_test_vector() {
    assert_eq!(
        Pkce::challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let p = Pkce::generate();
    assert!((43..=128).contains(&p.verifier.len()));
    assert_eq!(p.challenge, Pkce::challenge_for(&p.verifier));
    assert_ne!(Pkce::generate().verifier, p.verifier);
}

#[test]
fn authorization_url_has_all_required_parameters() {
    let c = OAuthClient::google("client-123", None);
    let pkce = Pkce::generate();
    let url =
        url::Url::parse(&c.authorization_url("http://127.0.0.1:5555", &pkce, "st&ate")).unwrap();
    let q: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(url.host_str(), Some("accounts.google.com"));
    assert_eq!(q["client_id"], "client-123");
    assert_eq!(q["redirect_uri"], "http://127.0.0.1:5555");
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["scope"], "https://www.googleapis.com/auth/drive.file");
    assert_eq!(q["code_challenge"], pkce.challenge);
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["state"], "st&ate", "state is URL-encoded and survives");
    assert_eq!(q["access_type"], "offline");
}

// ------------------------------------------------------------------------ tokens

#[tokio::test]
async fn exchanges_code_with_verifier_and_secret() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=the-code"))
        .and(body_string_contains("code_verifier=verif"))
        .and(body_string_contains("client_secret=secret-xyz"))
        .and(body_string_contains(
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A9",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at-1", "expires_in": 3599, "refresh_token": "rt-1",
            "scope": "https://www.googleapis.com/auth/drive.file", "token_type": "Bearer"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let pkce = Pkce {
        verifier: "verif".into(),
        challenge: "x".into(),
    };
    let t = oauth(&server)
        .exchange_code(&http(), "the-code", &pkce, "http://127.0.0.1:9")
        .await
        .unwrap();
    assert_eq!(t.access_token, "at-1");
    assert_eq!(t.refresh_token.as_deref(), Some("rt-1"));
    assert!(!t.needs_refresh());
}

#[tokio::test]
async fn exchange_without_refresh_token_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "access_token": "a", "expires_in": 3599 })),
        )
        .mount(&server)
        .await;
    let pkce = Pkce::generate();
    let err = oauth(&server)
        .exchange_code(&http(), "c", &pkce, "http://x")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::OAuth(_)), "{err:?}");
}

#[tokio::test]
async fn refresh_keeps_existing_refresh_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=rt-1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "access_token": "at-2", "expires_in": 3599 })),
        )
        .mount(&server)
        .await;
    let t = oauth(&server).refresh(&http(), "rt-1").await.unwrap();
    assert_eq!(t.access_token, "at-2");
    assert_eq!(t.refresh_token.as_deref(), Some("rt-1"));
}

#[tokio::test]
async fn invalid_grant_means_session_expired() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant", "error_description": "Token has been expired or revoked."
        })))
        .mount(&server)
        .await;
    let err = oauth(&server).refresh(&http(), "old").await.unwrap_err();
    assert!(matches!(err, Error::SessionExpired), "{err:?}");
}

#[tokio::test]
async fn other_oauth_errors_are_reported_with_description() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": "invalid_client", "error_description": "The OAuth client was not found."
        })))
        .mount(&server)
        .await;
    let err = oauth(&server).refresh(&http(), "rt").await.unwrap_err();
    assert!(
        err.to_string()
            .contains("invalid_client: The OAuth client was not found."),
        "{err}"
    );
}

#[tokio::test]
async fn revoke_treats_already_invalid_tokens_as_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/revoke"))
        .and(body_string_contains("token=rt-1"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&server)
        .await;
    oauth(&server).revoke(&http(), "rt-1").await.unwrap();
}

// ---------------------------------------------------------------------- loopback

#[tokio::test]
async fn loopback_receives_code_and_ignores_unrelated_requests() {
    let rx = LoopbackReceiver::bind().await.unwrap();
    let base = rx.redirect_uri();
    assert!(base.starts_with("http://127.0.0.1:"));
    let waiter = tokio::spawn(async move { rx.wait_for_code("good-state").await });

    let favicon = http()
        .get(format!("{base}/favicon.ico"))
        .send()
        .await
        .unwrap();
    assert_eq!(favicon.status(), 404);

    let resp = http()
        .get(format!("{base}/?state=good-state&code=4%2F0AbC&scope=x"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(
        resp.text()
            .await
            .unwrap()
            .contains("You can close this tab")
    );
    assert_eq!(waiter.await.unwrap().unwrap(), "4/0AbC");
}

#[tokio::test]
async fn loopback_reports_denied_consent() {
    let rx = LoopbackReceiver::bind().await.unwrap();
    let base = rx.redirect_uri();
    let waiter = tokio::spawn(async move { rx.wait_for_code("s").await });
    http()
        .get(format!("{base}/?error=access_denied&state=s"))
        .send()
        .await
        .unwrap();
    assert!(matches!(waiter.await.unwrap(), Err(Error::AccessDenied(e)) if e == "access_denied"));
}

#[tokio::test]
async fn loopback_rejects_wrong_state() {
    let rx = LoopbackReceiver::bind().await.unwrap();
    let base = rx.redirect_uri();
    let waiter = tokio::spawn(async move { rx.wait_for_code("expected").await });
    let resp = http()
        .get(format!("{base}/?code=c&state=forged"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    assert!(matches!(waiter.await.unwrap(), Err(Error::OAuth(_))));
}

// ------------------------------------------------------------------------- drive

#[tokio::test]
async fn about_returns_the_account() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/about"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "user": { "displayName": "Tux", "emailAddress": "tux@example.com" }
        })))
        .mount(&server)
        .await;
    let user = drive(&server).await.about("tok").await.unwrap();
    assert_eq!(user.email_address, "tux@example.com");
}

#[tokio::test]
async fn reuses_existing_backup_folder() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "files": [{ "id": "folder-1", "name": "TuxNotes Backups" }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let id = drive(&server)
        .await
        .find_or_create_folder("tok", "TuxNotes Backups")
        .await
        .unwrap();
    assert_eq!(id, "folder-1");
}

#[tokio::test]
async fn creates_backup_folder_when_missing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "files": [] })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/drive/v3/files"))
        .and(body_partial_json(json!({
            "name": "TuxNotes Backups", "mimeType": "application/vnd.google-apps.folder"
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "id": "new-folder", "name": "x" })),
        )
        .expect(1)
        .mount(&server)
        .await;
    let id = drive(&server)
        .await
        .find_or_create_folder("tok", "TuxNotes Backups")
        .await
        .unwrap();
    assert_eq!(id, "new-folder");
}

#[tokio::test]
async fn list_follows_pages_and_parses_string_sizes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param("pageToken", "p2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "files": [{ "id": "b", "name": "b.zip", "size": "20" }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "nextPageToken": "p2",
            "files": [{ "id": "a", "name": "a.zip", "size": "1048576",
                        "appProperties": { "tuxnotes": "backup", "noteCount": "12" } }]
        })))
        .mount(&server)
        .await;
    let files = drive(&server)
        .await
        .list_files("tok", "folder-1", "tuxnotes", "backup")
        .await
        .unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].size, Some(1_048_576));
    assert_eq!(files[0].app_properties["noteCount"], "12");
    assert_eq!(files[1].id, "b");
}

#[tokio::test]
async fn upload_uses_a_resumable_session() {
    let server = MockServer::start().await;
    let session = format!("{}/upload-session/abc", server.uri());
    Mock::given(method("POST"))
        .and(path("/upload/drive/v3/files"))
        .and(query_param("uploadType", "resumable"))
        .and(header("x-upload-content-length", "5"))
        .and(body_partial_json(json!({
            "name": "b.zip", "parents": ["folder-1"], "appProperties": { "tuxnotes": "backup" }
        })))
        .respond_with(ResponseTemplate::new(200).insert_header("Location", session.as_str()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/upload-session/abc"))
        .and(body_string_contains("hello"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "id": "file-1", "name": "b.zip", "size": "5" })),
        )
        .expect(1)
        .mount(&server)
        .await;
    let props = BTreeMap::from([("tuxnotes".to_string(), "backup".to_string())]);
    let file = drive(&server)
        .await
        .upload(
            "tok",
            "folder-1",
            "b.zip",
            "application/zip",
            b"hello".to_vec(),
            &props,
        )
        .await
        .unwrap();
    assert_eq!(file.id, "file-1");
    assert_eq!(file.size, Some(5));
}

#[tokio::test]
async fn upload_restarts_after_a_server_error() {
    let server = MockServer::start().await;
    let session = format!("{}/s", server.uri());
    Mock::given(method("POST"))
        .and(path("/upload/drive/v3/files"))
        .respond_with(ResponseTemplate::new(200).insert_header("Location", session.as_str()))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "ok", "name": "b" })))
        .mount(&server)
        .await;
    let file = drive(&server)
        .await
        .upload(
            "tok",
            "f",
            "b",
            "application/zip",
            vec![1, 2, 3],
            &BTreeMap::new(),
        )
        .await
        .unwrap();
    assert_eq!(file.id, "ok");
}

#[tokio::test]
async fn download_returns_bytes_and_enforces_limit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files/file-1"))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"zip-bytes".to_vec()))
        .mount(&server)
        .await;
    let d = drive(&server).await;
    assert_eq!(
        d.download("tok", "file-1", 100).await.unwrap(),
        b"zip-bytes"
    );
    assert!(matches!(
        d.download("tok", "file-1", 3).await,
        Err(Error::TooLarge)
    ));
}

#[tokio::test]
async fn retries_transient_errors_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "user": {} })))
        .mount(&server)
        .await;
    drive(&server).await.about("tok").await.unwrap();
}

#[tokio::test]
async fn gives_up_after_three_attempts() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .expect(3)
        .mount(&server)
        .await;
    let err = drive(&server).await.about("tok").await.unwrap_err();
    assert!(matches!(err, Error::Api { status: 503, .. }), "{err:?}");
}

#[tokio::test]
async fn maps_unauthorized_and_api_messages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/about"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1) // never retried
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": { "code": 403, "message": "The user's Drive storage quota has been exceeded." }
        })))
        .mount(&server)
        .await;
    let d = drive(&server).await;
    assert!(matches!(d.about("tok").await, Err(Error::Unauthorized)));
    let err = d.list_files("tok", "f", "k", "v").await.unwrap_err();
    assert!(
        err.to_string().contains("storage quota has been exceeded"),
        "{err}"
    );
}

#[tokio::test]
async fn delete_ignores_missing_files() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/drive/v3/files/gone"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    drive(&server).await.delete("tok", "gone").await.unwrap();
}
