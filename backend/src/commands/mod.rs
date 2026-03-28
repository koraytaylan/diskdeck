//! # Tauri IPC command handlers
//!
//! This module organizes all Tauri `#[tauri::command]` functions into thematic
//! submodules. Each submodule corresponds to a group of related operations
//! exposed to the frontend via Tauri's IPC invoke mechanism.
//!
//! ## Submodules
//!
//! - [`disk`] — CRUD operations for disk configurations (create, list, update,
//!   delete). Also contains the backend factory functions that instantiate the
//!   correct [`StorageBackend`](crate::storage::StorageBackend) from a
//!   [`DiskConfig`](crate::models::disk::DiskConfig).
//! - [`file`] — File and directory operations (list, read, write, copy, move,
//!   delete, rename, create folder). Supports bulk operations with progress
//!   events and cooperative cancellation.
//! - [`search`] — Full-text search with FTS5 index fast-path and live-search
//!   fallback for unindexed disks. Also exposes reindexing and index status.
//! - [`preferences`] — Simple key-value preference storage (theme, layout, etc.).
//! - [`bookmarks`] — Pinned path bookmark management (list, add, remove).
//! - [`profiles`] — Export/import disk configurations for team sharing.
//! - [`archive`] — Browse and extract .zip/.tar.gz archives.
//! - [`diff`] — Compare two directories across backends and show differences.
//!
//! ## IPC conventions
//!
//! All commands:
//! - Accept `State<'_, AppState>` for shared state access.
//! - Return `Result<T, DiskDeckError>` where `T` is serializable to JSON.
//! - Are registered in `lib.rs` via `tauri::generate_handler!`.

pub mod archive;
pub mod bookmarks;
pub mod diff;
pub mod disk;
pub mod file;
pub mod preferences;
pub mod profiles;
pub mod search;

