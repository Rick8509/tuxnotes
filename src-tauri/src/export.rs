//! Export (Markdown, PDF, print) and Markdown import.
//! The Markdown text itself is produced/parsed in the frontend with the editor's content
//! model; this module handles files, dialogs, attachments and printing.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;
use tuxnotes_core::Attachment;
use tuxnotes_core::attachments::{
    MAX_ATTACHMENT_BYTES, is_valid_id, sanitize_filename, sniff_image,
};

use crate::AppState;
use crate::attachments::percent_decode;

type CmdResult<T> = Result<T, String>;

const MAX_IMPORT_BYTES: u64 = 5 * 1024 * 1024;
const LINK_PREFIX: &str = "attachment:";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Markdown files picked for import in this session. Only these may pull in images.
#[derive(Default)]
pub struct ImportSession(Mutex<HashSet<PathBuf>>);

// --------------------------------------------------------------- pure helpers

/// Encode characters that would break a Markdown link destination.
fn md_path(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '(' => "%28".to_string(),
            ')' => "%29".to_string(),
            '<' => "%3C".to_string(),
            '>' => "%3E".to_string(),
            '#' => "%23".to_string(),
            '%' => "%25".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// Rewrite `attachment:<id>` links to `<folder>/<file name>` and return which attachment
/// goes to which (unique) file name. Unknown ids are left untouched.
pub fn rewrite_attachment_links(
    markdown: &str,
    folder: &str,
    filename_of: impl Fn(&str) -> Option<String>,
) -> (String, Vec<(String, String)>) {
    let mut out = String::with_capacity(markdown.len());
    let mut files: Vec<(String, String)> = Vec::new();
    let mut assigned: HashMap<String, String> = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    let mut rest = markdown;
    while let Some(i) = rest.find(LINK_PREFIX) {
        out.push_str(&rest[..i]);
        let after = &rest[i + LINK_PREFIX.len()..];
        let id = after.get(..36).unwrap_or("");
        let name = if is_valid_id(id) {
            assigned.get(id).cloned().or_else(|| {
                let original = filename_of(id)?;
                let unique = unique_name(&original, &mut used);
                assigned.insert(id.to_string(), unique.clone());
                files.push((id.to_string(), unique.clone()));
                Some(unique)
            })
        } else {
            None
        };
        match name {
            Some(name) => {
                out.push_str(&md_path(&format!("{folder}/{name}")));
                rest = &after[36..];
            }
            None => {
                out.push_str(LINK_PREFIX);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    (out, files)
}

/// "photo.png", then "photo (2).png", "photo (3).png" …
fn unique_name(name: &str, used: &mut HashSet<String>) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut candidate = name.to_string();
    let mut n = 2;
    while !used.insert(candidate.to_lowercase()) {
        candidate = format!("{stem} ({n}){ext}");
        n += 1;
    }
    candidate
}

/// The image file a Markdown file refers to, if it's safely inside the Markdown file's folder.
pub fn resolve_import_path(markdown_file: &Path, src: &str) -> Option<PathBuf> {
    let src = percent_decode(src.trim());
    if src.is_empty() || src.starts_with('/') || src.contains('\0') || src.contains("://") {
        return None;
    }
    let base = markdown_file.parent()?.canonicalize().ok()?;
    let candidate = base.join(&src).canonicalize().ok()?;
    (candidate.starts_with(&base) && candidate.is_file()).then_some(candidate)
}

/// A file name for an exported note, from its title.
fn export_stem(title: &str) -> String {
    let stem = sanitize_filename(&title.trim().replace(['/', '\\'], "-"));
    if stem == "attachment" || stem.is_empty() {
        "Note".into()
    } else {
        stem
    }
}

fn with_extension(path: PathBuf, ext: &str) -> PathBuf {
    let has = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext));
    if has {
        path
    } else {
        let mut s = path.into_os_string();
        s.push(".");
        s.push(ext);
        PathBuf::from(s)
    }
}

async fn ask_save_path(
    app: &AppHandle,
    title: &str,
    name: String,
    filter: (&str, &[&str]),
) -> CmdResult<Option<PathBuf>> {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .set_file_name(name)
        .add_filter(filter.0, filter.1)
        .save_file(move |p| {
            let _ = tx.send(p);
        });
    match rx.await.map_err(err)? {
        Some(p) => Ok(Some(p.into_path().map_err(err)?)),
        None => Ok(None),
    }
}

// ------------------------------------------------------------------ commands

/// Save a note as Markdown. Attachments are copied into a "<name> files" folder next to
/// the .md file and the links point there. Returns the saved path, or None if cancelled.
#[tauri::command]
pub async fn export_markdown(
    app: AppHandle,
    title: String,
    markdown: String,
) -> CmdResult<Option<String>> {
    let stem = export_stem(&title);
    let Some(path) = ask_save_path(
        &app,
        "Export as Markdown",
        format!("{stem}.md"),
        ("Markdown", &["md"]),
    )
    .await?
    else {
        return Ok(None);
    };
    let path = with_extension(path, "md");
    let folder_name = format!(
        "{} files",
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or(stem)
    );

    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (text, copies) = {
            let store = state.store.lock().map_err(|_| "note store unavailable")?;
            let (text, files) = rewrite_attachment_links(&markdown, &folder_name, |id| {
                store.get_attachment(id).ok().map(|a| a.filename)
            });
            let copies: Vec<(PathBuf, String)> = files
                .into_iter()
                .filter_map(|(id, name)| {
                    Some((
                        store.attachment_path(&state.attachments_dir, &id).ok()?,
                        name,
                    ))
                })
                .collect();
            (text, copies)
        };
        if !copies.is_empty() {
            let folder = path.with_file_name(&folder_name);
            fs::create_dir_all(&folder).map_err(err)?;
            for (source, name) in copies {
                fs::copy(&source, folder.join(&name))
                    .map_err(|e| format!("Couldn't copy “{name}”: {e}"))?;
            }
        }
        let tmp = path.with_extension("md.partial");
        fs::write(&tmp, text).map_err(err)?;
        fs::rename(&tmp, &path).map_err(err)?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(err)?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkdownFile {
    path: String,
    name: String,
    text: String,
}

/// Pick Markdown files to import. Returns their text; images are imported separately.
#[tauri::command]
pub async fn pick_markdown_files(
    app: AppHandle,
    session: State<'_, ImportSession>,
) -> CmdResult<Vec<MarkdownFile>> {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .file()
        .set_title("Import Markdown Files")
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .pick_files(move |files| {
            let _ = tx.send(files);
        });
    let Some(files) = rx.await.map_err(err)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut picked = session.0.lock().map_err(|_| "import unavailable")?;
    for path in files.into_iter().filter_map(|f| f.into_path().ok()) {
        let size = fs::metadata(&path).map_err(err)?.len();
        if size > MAX_IMPORT_BYTES {
            return Err(format!(
                "“{}” is too large to import (the limit is 5 MB).",
                path.display()
            ));
        }
        let text = String::from_utf8_lossy(&fs::read(&path).map_err(err)?).into_owned();
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        picked.insert(path.clone());
        out.push(MarkdownFile {
            path: path.display().to_string(),
            name,
            text,
        });
    }
    Ok(out)
}

/// Attach an image referenced by an imported Markdown file (relative path, inside its folder).
#[tauri::command]
pub fn import_image(
    state: State<'_, AppState>,
    session: State<'_, ImportSession>,
    note_id: String,
    markdown_path: String,
    src: String,
) -> CmdResult<Attachment> {
    let markdown_path = PathBuf::from(markdown_path);
    if !session
        .0
        .lock()
        .map_err(|_| "import unavailable")?
        .contains(&markdown_path)
    {
        return Err("That file wasn't chosen for import.".into());
    }
    let path = resolve_import_path(&markdown_path, &src)
        .ok_or_else(|| format!("“{src}” isn't next to the imported file."))?;
    if fs::metadata(&path).map_err(err)?.len() > MAX_ATTACHMENT_BYTES {
        return Err(format!("“{src}” is too large to attach."));
    }
    let bytes = fs::read(&path).map_err(err)?;
    if sniff_image(&bytes).is_none() {
        return Err(format!("“{src}” isn't an image."));
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    state
        .store
        .lock()
        .map_err(|_| "note store unavailable")?
        .add_attachment(&state.attachments_dir, Some(&note_id), &name, "", &bytes)
        .map_err(err)
}

/// Open the system print dialog for the open note (print styles show only the note).
#[tauri::command]
pub fn print_note(window: WebviewWindow) -> CmdResult<()> {
    window.print().map_err(err)
}

/// Save the open note as a PDF without a print dialog. Returns the path, or None if cancelled.
#[tauri::command]
pub async fn export_pdf(
    app: AppHandle,
    window: WebviewWindow,
    title: String,
) -> CmdResult<Option<String>> {
    let stem = export_stem(&title);
    let Some(path) = ask_save_path(
        &app,
        "Export as PDF",
        format!("{stem}.pdf"),
        ("PDF", &["pdf"]),
    )
    .await?
    else {
        return Ok(None);
    };
    let path = with_extension(path, "pdf");
    print_to_pdf(&window, &path).await?;
    Ok(Some(path.display().to_string()))
}

#[cfg(target_os = "linux")]
async fn print_to_pdf(window: &WebviewWindow, path: &Path) -> CmdResult<()> {
    use webkit2gtk::PrintOperationExt;

    let uri = tauri::Url::from_file_path(path)
        .map_err(|_| "Invalid file path".to_string())?
        .to_string();
    let (tx, rx) = oneshot::channel::<CmdResult<()>>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    window
        .with_webview(move |webview| {
            let settings = gtk::PrintSettings::new();
            settings.set_printer("Print to File");
            settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
            settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(&uri));
            let op = webkit2gtk::PrintOperation::new(&webview.inner());
            op.set_print_settings(&settings);
            let done = tx.clone();
            op.connect_finished(move |_| {
                if let Some(tx) = done.lock().ok().and_then(|mut t| t.take()) {
                    let _ = tx.send(Ok(()));
                }
            });
            let failed = tx.clone();
            op.connect_failed(move |_, e| {
                if let Some(tx) = failed.lock().ok().and_then(|mut t| t.take()) {
                    let _ = tx.send(Err(format!("Couldn't create the PDF: {e}")));
                }
            });
            op.print();
        })
        .map_err(err)?;
    rx.await.map_err(err)?
}

#[cfg(not(target_os = "linux"))]
async fn print_to_pdf(_window: &WebviewWindow, _path: &Path) -> CmdResult<()> {
    Err("PDF export is only available on Linux. Use Print… instead.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "01a1f363-bf56-7331-84f9-eab2eff398df";
    const B: &str = "01a1f363-bf56-7331-84f9-eab2eff398e0";
    const C: &str = "01a1f363-bf56-7331-84f9-eab2eff398e1";

    #[test]
    fn attachment_links_point_into_the_files_folder() {
        let md = format!(
            "![map](attachment:{A})\n[📎 plan.pdf](attachment:{B})\n![again](attachment:{A})\n![other](attachment:{C})\nattachment: plain text\n"
        );
        let names = HashMap::from([(A, "map (old).png"), (B, "plan.pdf"), (C, "MAP (OLD).png")]);
        let (out, files) =
            rewrite_attachment_links(&md, "Trip files", |id| names.get(id).map(|s| s.to_string()));
        assert!(
            out.contains("![map](Trip%20files/map%20%28old%29.png)"),
            "{out}"
        );
        assert!(out.contains("[📎 plan.pdf](Trip%20files/plan.pdf)"));
        assert!(
            out.contains("![again](Trip%20files/map%20%28old%29.png)"),
            "same attachment, same file"
        );
        assert!(
            out.contains("![other](Trip%20files/MAP%20%28OLD%29%20%282%29.png)"),
            "name clash gets a number: {out}"
        );
        assert!(
            out.contains("attachment: plain text"),
            "non-links untouched"
        );
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn unknown_attachments_are_left_alone() {
        let md = format!("![gone](attachment:{A})");
        let (out, files) = rewrite_attachment_links(&md, "x files", |_| None);
        assert_eq!(out, md);
        assert!(files.is_empty());
    }

    #[test]
    fn imported_images_must_stay_inside_the_markdown_folder() {
        let dir = std::env::temp_dir().join(format!("tuxnotes-import-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("notes/img")).unwrap();
        fs::write(dir.join("notes/img/a b.png"), b"x").unwrap();
        fs::write(dir.join("secret.txt"), b"x").unwrap();
        let md = dir.join("notes/readme.md");
        fs::write(&md, b"").unwrap();
        assert!(resolve_import_path(&md, "img/a%20b.png").is_some());
        assert!(resolve_import_path(&md, "./img/a b.png").is_some());
        assert!(
            resolve_import_path(&md, "../secret.txt").is_none(),
            "no escaping the folder"
        );
        assert!(
            resolve_import_path(&md, "/etc/passwd").is_none(),
            "no absolute paths"
        );
        assert!(resolve_import_path(&md, "file:///etc/passwd").is_none());
        assert!(resolve_import_path(&md, "img/missing.png").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_names_come_from_the_title() {
        assert_eq!(export_stem("Trip: Rome/Paris"), "Trip_ Rome-Paris");
        assert_eq!(export_stem("Groceries"), "Groceries");
        assert_eq!(export_stem("  "), "Note");
        assert_eq!(
            with_extension(PathBuf::from("/x/a"), "md"),
            PathBuf::from("/x/a.md")
        );
        assert_eq!(
            with_extension(PathBuf::from("/x/a.MD"), "md"),
            PathBuf::from("/x/a.MD")
        );
    }
}
