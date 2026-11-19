//! Attachments in the app: the `attachment://` protocol that serves files to the editor,
//! and the commands to add, open and save them. Storage rules live in tuxnotes-core.

use std::path::PathBuf;

use serde::Serialize;
use tauri::http::{Response, StatusCode, header};
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager, State, UriSchemeContext};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::oneshot;
use tuxnotes_core::Attachment;
use tuxnotes_core::attachments::is_valid_id;

use crate::AppState;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ------------------------------------------------------------------- protocol

/// Serve `attachment://localhost/<id>` to the webview. Only ids that exist in the
/// database are served, so no other file on disk can be reached. Non-image files are
/// sent as plain downloads, and every response is sandboxed.
pub fn handle_protocol<R: tauri::Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    let id = request.uri().path().trim_start_matches('/').to_string();
    tauri::async_runtime::spawn_blocking(move || responder.respond(serve(&app, &id)));
}

fn serve<R: tauri::Runtime>(app: &AppHandle<R>, id: &str) -> Response<Vec<u8>> {
    let not_found = || {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .expect("static response")
    };
    if !is_valid_id(id) {
        return not_found();
    }
    let state = app.state::<AppState>();
    let found = state.store.lock().ok().and_then(|store| {
        let attachment = store.get_attachment(id).ok()?;
        let path = store.attachment_path(&state.attachments_dir, id).ok()?;
        Some((attachment, path))
    });
    let Some((attachment, path)) = found else {
        return not_found();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return not_found();
    };
    let content_type = if attachment.is_image() {
        attachment.mime.as_str()
    } else {
        "application/octet-stream"
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header("X-Content-Type-Options", "nosniff")
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; sandbox",
        )
        // Attachments never change once added, so the webview may cache them.
        .header(
            header::CACHE_CONTROL,
            "private, max-age=31536000, immutable",
        )
        .body(bytes)
        .unwrap_or_else(|_| not_found())
}

// ------------------------------------------------------------------- commands

/// Decode a percent-encoded header value (file names may contain any characters).
pub(crate) fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = value.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Add pasted or dropped file data. The bytes arrive as the raw request body; the note id,
/// file name and type come in `x-note-id`, `x-filename` and `x-mime` headers (percent-encoded).
#[tauri::command]
pub fn add_attachment(state: State<'_, AppState>, request: Request<'_>) -> CmdResult<Attachment> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("No file data was received.".into());
    };
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(percent_decode)
            .filter(|v| !v.is_empty())
    };
    let store = state.store.lock().map_err(|_| "note store unavailable")?;
    store
        .add_attachment(
            &state.attachments_dir,
            header("x-note-id").as_deref(),
            &header("x-filename").unwrap_or_default(),
            &header("x-mime").unwrap_or_default(),
            bytes,
        )
        .map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachResult {
    attachments: Vec<Attachment>,
    /// Files that couldn't be added (e.g. too large), as user-facing messages.
    errors: Vec<String>,
}

/// Show the system file picker and attach the chosen files.
#[tauri::command]
pub async fn attach_files(app: AppHandle, note_id: String) -> CmdResult<AttachResult> {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .file()
        .set_title("Attach Files")
        .pick_files(move |files| {
            let _ = tx.send(files);
        });
    let Some(files) = rx.await.map_err(err)? else {
        return Ok(AttachResult {
            attachments: Vec::new(),
            errors: Vec::new(),
        });
    };
    let paths: Vec<PathBuf> = files
        .into_iter()
        .filter_map(|f| f.into_path().ok())
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().map_err(|_| "note store unavailable")?;
        let mut result = AttachResult {
            attachments: Vec::new(),
            errors: Vec::new(),
        };
        for path in paths {
            match store.add_attachment_from_file(&state.attachments_dir, Some(&note_id), &path) {
                Ok(a) => result.attachments.push(a),
                Err(e) => result.errors.push(e.to_string()),
            }
        }
        Ok(result)
    })
    .await
    .map_err(err)?
}

/// Open an attachment with the default app for its type.
#[tauri::command]
pub fn open_attachment(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let path = state
        .store
        .lock()
        .map_err(|_| "note store unavailable")?
        .attachment_path(&state.attachments_dir, &id)
        .map_err(err)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| format!("Couldn't open the file: {e}"))
}

/// Ask where to save a copy of an attachment, then copy it there. Returns whether it was saved.
#[tauri::command]
pub async fn save_attachment_as(app: AppHandle, id: String) -> CmdResult<bool> {
    let (attachment, source) = {
        let state = app.state::<AppState>();
        let store = state.store.lock().map_err(|_| "note store unavailable")?;
        let attachment = store.get_attachment(&id).map_err(err)?;
        let source = store
            .attachment_path(&state.attachments_dir, &id)
            .map_err(err)?;
        (attachment, source)
    };
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .file()
        .set_title("Save a Copy")
        .set_file_name(&attachment.filename)
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(target) = rx.await.map_err(err)? else {
        return Ok(false);
    };
    let target = target.into_path().map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || std::fs::copy(&source, &target))
        .await
        .map_err(err)?
        .map_err(|e| format!("Couldn't save the file: {e}"))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::percent_decode;

    #[test]
    fn decodes_percent_encoded_file_names() {
        assert_eq!(percent_decode("caf%C3%A9%20menu.pdf"), "café menu.pdf");
        assert_eq!(percent_decode("plain.png"), "plain.png");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("bad%zzescape"), "bad%zzescape");
    }
}
