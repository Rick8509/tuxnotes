//! A one-shot HTTP listener on 127.0.0.1 that receives Google's OAuth redirect.

use std::collections::HashMap;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

use crate::error::{Error, Result};

const MAX_REQUEST_HEAD: usize = 16 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(10);

pub struct LoopbackReceiver {
    listener: TcpListener,
    port: u16,
}

impl LoopbackReceiver {
    /// Listen on a random free port on the loopback interface.
    pub async fn bind() -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        Ok(Self { listener, port })
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Wait until the browser is redirected back with an authorization code.
    /// Unrelated requests (e.g. `/favicon.ico`) are answered and ignored.
    /// Wrap this in a timeout / cancellation at the call site.
    pub async fn wait_for_code(self, expected_state: &str) -> Result<String> {
        loop {
            let (mut socket, _) = self.listener.accept().await?;
            let Ok(Ok(Some(target))) =
                tokio::time::timeout(READ_TIMEOUT, read_request_target(&mut socket)).await
            else {
                continue;
            };
            let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
                respond(&mut socket, 400, "Bad request", "").await;
                continue;
            };
            if url.path() != "/" {
                respond(&mut socket, 404, "Not found", "").await;
                continue;
            }
            let params: HashMap<String, String> = url.query_pairs().into_owned().collect();

            if let Some(error) = params.get("error") {
                respond(
                    &mut socket,
                    200,
                    "Sign-in cancelled",
                    "TuxNotes was not connected to Google Drive. You can close this tab.",
                )
                .await;
                return Err(Error::AccessDenied(error.clone()));
            }
            match (params.get("code"), params.get("state")) {
                (Some(code), Some(state)) if state == expected_state => {
                    respond(
                        &mut socket,
                        200,
                        "TuxNotes is connected",
                        "You're signed in to Google Drive. You can close this tab and return to TuxNotes.",
                    )
                    .await;
                    return Ok(code.clone());
                }
                (Some(_), _) => {
                    respond(
                        &mut socket,
                        400,
                        "Sign-in failed",
                        "The sign-in response didn't match this request. Please try again from TuxNotes.",
                    )
                    .await;
                    return Err(Error::OAuth("state mismatch in redirect".into()));
                }
                _ => respond(&mut socket, 400, "Bad request", "").await,
            }
        }
    }
}

/// Read the request line and return its target (path + query).
async fn read_request_target(socket: &mut TcpStream) -> std::io::Result<Option<String>> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = socket.read(&mut chunk).await?;
        if n == 0 || buf.len() + n > MAX_REQUEST_HEAD {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf);
    let mut parts = head.lines().next().unwrap_or("").split_whitespace();
    Ok(match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) if target.starts_with('/') => Some(target.to_string()),
        _ => None,
    })
}

async fn respond(socket: &mut TcpStream, status: u16, title: &str, message: &str) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Bad Request",
    };
    let body = page(title, message);
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
}

fn page(title: &str, message: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>{title}</title>
<style>body{{font-family:system-ui,sans-serif;display:grid;place-items:center;height:100vh;margin:0;background:#f5f5f7;color:#1d1d1f}}
main{{text-align:center;max-width:420px;padding:32px;background:#fff;border-radius:14px;box-shadow:0 8px 30px rgba(0,0,0,.08)}}
h1{{font-size:20px;margin:0 0 8px}}p{{color:#6e6e73;margin:0}}
@media (prefers-color-scheme:dark){{body{{background:#1c1c1e;color:#f5f5f7}}main{{background:#2c2c2e}}p{{color:#a1a1a6}}}}</style>
</head><body><main><h1>{title}</h1><p>{message}</p></main></body></html>"#
    )
}
