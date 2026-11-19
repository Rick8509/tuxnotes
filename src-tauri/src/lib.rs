mod attachments;
mod backup_service;
mod commands;
mod credentials;
mod google_client;
mod settings;

use std::sync::Mutex;
use std::time::Duration;

use backup_service::BackupService;
use tauri::{AppHandle, Emitter, Manager};
use tuxnotes_core::Store;

/// Shared application state. SQLite connections are not `Sync`, so the store
/// lives behind a mutex; every command holds it only for one short operation.
pub struct AppState {
    pub store: Mutex<Store>,
    pub backup: BackupService,
    /// `<data dir>/attachments`: one folder per attachment, named by its id.
    pub attachments_dir: std::path::PathBuf,
}

/// How often the scheduler checks whether an automatic backup is due.
const AUTO_BACKUP_CHECK: Duration = Duration::from_secs(15 * 60);
/// Give the app time to settle before the first automatic backup.
const AUTO_BACKUP_INITIAL_DELAY: Duration = Duration::from_secs(90);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        // Must be first: a second launch just focuses the running window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        // Remember window size, position and maximized state between launches.
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // Serves note images/files to the editor by id (see attachments.rs).
        .register_asynchronous_uri_scheme_protocol("attachment", attachments::handle_protocol)
        .setup(|app| {
            // ~/.local/share/<identifier>/notes.db and ~/.config/<identifier>/settings.json
            let data_dir = app.path().app_data_dir()?;
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("notes.db");
            log::info!("opening database at {}", db_path.display());

            let store = Store::open(&db_path)?;
            match store.purge_expired_trash() {
                Ok(0) => {}
                Ok(n) => log::info!("purged {n} expired note(s) from Recently Deleted"),
                Err(e) => log::warn!("could not purge expired trash: {e}"),
            }
            match store.discard_blank_notes() {
                Ok(0) => {}
                Ok(n) => log::info!("removed {n} empty note(s) left from last session"),
                Err(e) => log::warn!("could not clean up empty notes: {e}"),
            }
            let attachments_dir = data_dir.join("attachments");
            match store.collect_unused_attachments(
                &attachments_dir,
                backup_service::now_ms(),
                tuxnotes_core::attachments::UNUSED_GRACE_MS,
            ) {
                Ok(0) => {}
                Ok(n) => log::info!("removed {n} attachment(s) no note uses any more"),
                Err(e) => log::warn!("could not clean up attachments: {e}"),
            }
            let backup = BackupService::new(
                data_dir,
                config_dir,
                db_path,
                app.package_info().version.to_string(),
            );
            backup.clean_tmp();
            app.manage(AppState {
                store: Mutex::new(store),
                backup,
                attachments_dir,
            });
            spawn_auto_backup(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_folders,
            commands::create_folder,
            commands::rename_folder,
            commands::delete_folder,
            commands::list_notes,
            commands::get_note,
            commands::create_note,
            commands::update_note_content,
            commands::set_note_pinned,
            commands::move_note,
            commands::trash_note,
            commands::restore_note,
            commands::delete_note_permanently,
            commands::empty_trash,
            commands::trash_count,
            commands::search_notes,
            commands::discard_note_if_blank,
            commands::backup_status,
            commands::set_backup_preferences,
            commands::google_sign_in,
            commands::google_cancel_sign_in,
            commands::google_sign_out,
            commands::backup_now,
            commands::list_backups,
            commands::restore_backup,
            commands::app_info,
            attachments::add_attachment,
            attachments::attach_files,
            attachments::open_attachment,
            attachments::save_attachment_as,
        ])
        .run(tauri::generate_context!())
        .expect("error while running TuxNotes");
}

/// Background loop that runs automatic backups when they're due.
fn spawn_auto_backup(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(AUTO_BACKUP_INITIAL_DELAY).await;
        loop {
            let state = app.state::<AppState>();
            if state.backup.auto_backup_due(backup_service::now_ms()) {
                log::info!("starting automatic backup");
                match state.backup.backup_now(&app).await {
                    Ok(info) => {
                        log::info!("automatic backup finished: {}", info.name);
                        let _ = app.emit("backup-status-changed", ());
                    }
                    Err(e) => {
                        log::warn!("automatic backup failed: {e}");
                        let _ = app.emit("backup-status-changed", ());
                    }
                }
            }
            tokio::time::sleep(AUTO_BACKUP_CHECK).await;
        }
    });
}
