//! # DiskDeck Backend Library
//!
//! This is the main entry point for the DiskDeck Tauri application backend.
//! DiskDeck is a cross-platform desktop app that provides a unified file-browser
//! interface over multiple storage backends (local filesystem, AWS S3, Google Cloud
//! Storage, Azure Blob Storage, SFTP, and FTP).
//!
//! ## Architecture overview
//!
//! The backend is organized into the following modules:
//!
//! - **`commands`** — Tauri IPC command handlers exposed to the frontend (disk CRUD,
//!   file operations, search, preferences).
//! - **`storage`** — The [`StorageBackend`](storage::StorageBackend) trait and its
//!   implementations for each provider (local, S3, Azure, SFTP, FTP).
//! - **`db`** — SQLCipher-encrypted persistence layer for disk configs (including
//!   credentials), preferences, bookmarks, and the FTS5 full-text search index.
//! - **`models`** — Shared data structures serialized across the IPC boundary.
//! - **`indexer`** — Background indexing engine that walks all backends and populates
//!   the FTS5 search index.
//! - **`state`** — The shared [`AppState`](state::AppState) managed by Tauri, holding
//!   backends, disk configs, and the database handle.
//! - **`security`** — OS keychain integration for the database encryption key.
//! - **`error`** — Unified error type with safe-for-frontend serialization.
//!
//! ## Startup flow
//!
//! 1. Initialize logging.
//! 2. Open the SQLCipher-encrypted database and run migrations. If the database
//!    cannot be decrypted (wrong key or corrupt), it is deleted and recreated.
//! 3. Migrate any legacy `disks.json` file into the database.
//! 4. Restore saved disk configurations — synchronous backends (Local, Azure, SFTP,
//!    FTP) are initialized immediately; asynchronous backends (S3) are deferred to
//!    a background task. Credentials are read from the encrypted database on demand.
//! 5. Spawn background FTS5 indexing for all disks.
//! 6. Register all Tauri IPC command handlers.

mod commands;
mod db;
mod error;
mod indexer;
mod models;
mod security;
mod state;
mod storage;
pub mod watcher;

use tauri::Manager;

use commands::archive::{extract_archive, list_archive};
use commands::bookmarks::{add_bookmark, list_bookmarks, remove_bookmark};
use commands::diff::diff_directories;
use commands::profiles::{export_profiles, import_profiles};
use commands::disk::{create_disk, delete_disk, list_disks, update_disk};
use commands::file::{
    batch_rename, cancel_job, clear_finished_jobs, copy_entries, create_folder, cross_copy_entries,
    cross_move_entries, delete_entries, get_entry, get_folder_size, get_size_breakdown,
    list_entries, list_jobs, move_entries, read_file, rename_entry, write_file,
};
use commands::preferences::{get_preference, set_preference};
use commands::search::{get_index_status, reindex_disk, search_entries};
use watcher::{watch_directory, unwatch_directory};

/// Builds and runs the Tauri application.
///
/// This function is the single public API of the library crate. It performs all
/// startup initialization (database, keychain, backend restoration, indexing) and
/// then enters the Tauri event loop. It never returns under normal operation.
///
/// # Panics
///
/// Panics if:
/// - The app data directory cannot be resolved.
/// - The database cannot be opened or migrated.
/// - The Tauri runtime fails to start.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Use tracing-subscriber instead of env_logger so we capture logs from
    // crates that use the `tracing` facade.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
        )
        .init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");

            // The DB encryption key can come from two sources:
            //
            // 1. DISKDECK_DB_KEY env var — used during development to bypass the
            //    OS keychain entirely (avoids repeated permission prompts caused
            //    by binary cdhash changes on every recompilation).
            //
            // 2. OS keychain — used in production builds. The key is stored
            //    under "com.diskdeck.app" / "db-encryption-key" and created on
            //    first launch.
            std::fs::create_dir_all(&data_dir).ok();
            let db_key = if let Ok(key) = std::env::var("DISKDECK_DB_KEY") {
                log::info!("Using DB key from DISKDECK_DB_KEY environment variable");
                key
            } else {
                // Show a welcome dialog before the first keychain access
                let keychain_marker = data_dir.join(".keychain_ok");
                if !keychain_marker.exists() {
                    let proceed = rfd::MessageDialog::new()
                        .set_title("Welcome to DiskDeck")
                        .set_description(
                            "DiskDeck encrypts its local database to keep your file listings, \
                             bookmarks, and search indexes private. The encryption key is stored \
                             in your operating system's secure credential store.\n\n\
                             Your system may prompt you to authorize this access — this is a \
                             one-time setup and a standard security measure.\n\n\
                             Your data stays on your machine, protected from other applications."
                        )
                        .set_level(rfd::MessageLevel::Info)
                        .set_buttons(rfd::MessageButtons::OkCancelCustom(
                            "Continue".into(),
                            "Quit".into(),
                        ))
                        .show();

                    if proceed == rfd::MessageDialogResult::Cancel
                        || proceed == rfd::MessageDialogResult::Custom("Quit".into())
                    {
                        std::process::exit(0);
                    }
                }

                loop {
                    match security::keyring::get_or_create_db_key() {
                        Ok(key) => {
                            if !keychain_marker.exists() {
                                let _ = std::fs::write(&keychain_marker, "ok");
                            }
                            break key;
                        }
                        Err(e) => {
                            let msg = e.to_string();
                            if msg.contains("keychain_denied") {
                                let _ = std::fs::remove_file(&keychain_marker);

                                let retry = rfd::MessageDialog::new()
                                    .set_title("Keychain Access Required")
                                    .set_description(
                                        "DiskDeck was unable to access the secure credential store. \
                                         Without it, the app cannot encrypt its local database.\n\n\
                                         DiskDeck stores file listings, bookmarks, and search indexes \
                                         in an encrypted database to protect your privacy. The encryption \
                                         key is held in your operating system's secure credential store.\n\n\
                                         Please authorize access when prompted."
                                    )
                                    .set_level(rfd::MessageLevel::Warning)
                                    .set_buttons(rfd::MessageButtons::OkCancelCustom(
                                        "Try Again".into(),
                                        "Quit".into(),
                                    ))
                                    .show();

                                if retry == rfd::MessageDialogResult::Cancel
                                    || retry == rfd::MessageDialogResult::Custom("Quit".into())
                                {
                                    std::process::exit(0);
                                }
                            } else {
                                panic!("Failed to access OS keychain: {e}");
                            }
                        }
                    }
                }
            };

            // Open the encrypted SQLCipher database.
            let store = db::DiskStore::new(&data_dir, &db_key)
                .expect("failed to initialize disk store");

            // One-time migration from the legacy plain-text disks.json format.
            // On success the JSON file is deleted to prevent re-importing.
            let json_path = data_dir.join("disks.json");
            if json_path.exists() {
                match store.import_from_json(&json_path) {
                    Ok(count) if count > 0 => {
                        log::info!("Migrated {count} disks from disks.json to database");
                        let _ = std::fs::remove_file(&json_path);
                    }
                    Err(e) => {
                        log::warn!("Failed to migrate disks.json: {e}");
                    }
                    _ => {}
                }
            }

            let saved_disks = store.load().unwrap_or_default();

            let app_state = state::AppState::new(store);

            // Load disk configs into state but do NOT construct backends yet.
            // Backends are built lazily on first use (in get_backend) to avoid
            // accessing the OS keychain on app startup.
            *app_state.disks.blocking_write() = saved_disks;

            app.manage(app_state);

            // Kick off background FTS5 indexing for every registered disk.
            // The indexer will trigger lazy backend construction for each disk
            // it needs to walk.
            let index_handle = app.handle().clone();
            indexer::start_background_indexing(index_handle);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Disk CRUD commands
            list_disks,
            create_disk,
            update_disk,
            delete_disk,
            // File operation commands
            list_entries,
            get_entry,
            read_file,
            write_file,
            copy_entries,
            move_entries,
            cross_copy_entries,
            cross_move_entries,
            delete_entries,
            rename_entry,
            create_folder,
            get_folder_size,
            get_size_breakdown,
            batch_rename,
            cancel_job,
            list_jobs,
            clear_finished_jobs,
            // Search commands
            search_entries,
            reindex_disk,
            get_index_status,
            // Preference commands
            get_preference,
            set_preference,
            // Bookmark commands
            list_bookmarks,
            add_bookmark,
            remove_bookmark,
            // Profile commands
            export_profiles,
            import_profiles,
            // Archive commands
            list_archive,
            extract_archive,
            // Diff command
            diff_directories,
            // Watcher commands
            watch_directory,
            unwatch_directory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running DiskDeck");
}
