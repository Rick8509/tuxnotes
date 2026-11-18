mod commands;

use std::sync::Mutex;

use tauri::Manager;
use tuxnotes_core::Store;

/// Shared application state. SQLite connections are not `Sync`, so the store
/// lives behind a mutex; every command holds it only for one short operation.
pub struct AppState {
    pub store: Mutex<Store>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .setup(|app| {
            // ~/.local/share/<identifier>/notes.db
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("notes.db");
            log::info!("opening database at {}", db_path.display());

            let store = Store::open(&db_path)?;
            match store.purge_expired_trash() {
                Ok(0) => {}
                Ok(n) => log::info!("purged {n} expired note(s) from Recently Deleted"),
                Err(e) => log::warn!("could not purge expired trash: {e}"),
            }
            app.manage(AppState {
                store: Mutex::new(store),
            });
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running TuxNotes");
}
