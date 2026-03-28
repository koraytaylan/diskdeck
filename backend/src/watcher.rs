//! # Filesystem watcher for live directory updates
//!
//! Provides Tauri IPC commands to start and stop watching local directories
//! for changes. When a file is created, modified, or deleted in a watched
//! directory, an `"fs-change"` event is emitted to the frontend so the file
//! list can auto-refresh.
//!
//! ## Scope
//!
//! Filesystem watching only applies to the [`LocalBackend`](crate::storage::local::LocalBackend).
//! Remote backends (S3, Azure, SFTP, FTP) would require polling, which is
//! out of scope. Attempting to watch a non-local disk returns an error.
//!
//! ## Architecture
//!
//! Each call to [`watch_directory`] creates a `notify::RecommendedWatcher`
//! configured in non-recursive mode (only the immediate directory). The
//! watcher is stored in [`AppState::watchers`] keyed by a generated UUID
//! ("watch ID"). Calling [`unwatch_directory`] drops the watcher, which
//! stops the OS-level file system monitor.

use std::collections::HashMap;
use std::path::PathBuf;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;

use crate::error::DiskDeckError;
use crate::models::disk::DiskType;
use crate::state::AppState;

/// Payload emitted to the frontend when a filesystem change is detected.
#[derive(Debug, Clone, Serialize)]
pub struct FsChangeEvent {
    /// ID of the disk where the change occurred.
    pub disk_id: String,
    /// Storage-relative directory path being watched.
    pub path: String,
    /// Kind of change: "create", "modify", or "delete".
    pub kind: String,
}

/// Registry of active filesystem watchers, keyed by watch ID.
///
/// Each entry holds a `RecommendedWatcher` that will be dropped (and thus
/// stopped) when removed from the map.
pub struct WatcherRegistry {
    /// Map of watch ID to active watcher instance.
    watchers: RwLock<HashMap<String, RecommendedWatcher>>,
}

impl Default for WatcherRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl WatcherRegistry {
    /// Creates an empty watcher registry.
    pub fn new() -> Self {
        Self {
            watchers: RwLock::new(HashMap::new()),
        }
    }

    /// Inserts a watcher into the registry.
    pub async fn insert(&self, id: String, watcher: RecommendedWatcher) {
        self.watchers.write().await.insert(id, watcher);
    }

    /// Removes and drops a watcher, stopping filesystem monitoring.
    ///
    /// Returns `true` if the watcher was found and removed, `false` otherwise.
    pub async fn remove(&self, id: &str) -> bool {
        self.watchers.write().await.remove(id).is_some()
    }
}

/// Maps a `notify::EventKind` to a simple change type string.
///
/// Returns `Some("create")`, `Some("modify")`, or `Some("delete")` for
/// relevant event kinds, or `None` for events we don't care about
/// (access, metadata-only, etc.).
fn event_kind_to_string(kind: &EventKind) -> Option<&'static str> {
    match kind {
        EventKind::Create(_) => Some("create"),
        EventKind::Modify(_) => Some("modify"),
        EventKind::Remove(_) => Some("delete"),
        _ => None,
    }
}

/// Resolves the full filesystem path for a local disk's directory.
///
/// Reads the disk config from state, verifies it is a local backend,
/// and joins the configured root with the requested path.
///
/// # Errors
///
/// Returns `DiskDeckError::NotFound` if the disk does not exist.
/// Returns `DiskDeckError::Storage` if the disk is not a local backend.
async fn resolve_local_path(
    state: &AppState,
    disk_id: &str,
    path: &str,
) -> Result<PathBuf, DiskDeckError> {
    let disks = state.disks.read().await;
    let disk = disks
        .iter()
        .find(|d| d.id == disk_id)
        .ok_or_else(|| DiskDeckError::NotFound(format!("Disk '{}' not found", disk_id)))?;

    if disk.disk_type != DiskType::Local {
        return Err(DiskDeckError::Storage(
            "Watch mode is only supported for local disks".into(),
        ));
    }

    let root = disk
        .config
        .get("root")
        .and_then(|v| v.as_str())
        .ok_or_else(|| DiskDeckError::Storage("Local disk missing 'root' config".into()))?;

    let full_path = PathBuf::from(root).join(path.trim_start_matches('/'));
    Ok(full_path)
}

/// Starts watching a local directory for changes.
///
/// Creates an OS-level filesystem watcher that monitors the specified
/// directory (non-recursively). When files are created, modified, or deleted,
/// an `"fs-change"` event is emitted to the frontend.
///
/// # Errors
///
/// Returns an error if:
/// - The disk is not found.
/// - The disk is not a local backend.
/// - The OS watcher cannot be created.
#[tauri::command]
pub async fn watch_directory(
    app: AppHandle,
    state: State<'_, AppState>,
    disk_id: String,
    path: String,
) -> Result<String, DiskDeckError> {
    let full_path = resolve_local_path(&state, &disk_id, &path).await?;

    let watch_id = uuid::Uuid::new_v4().to_string();

    let app_clone = app.clone();
    let disk_id_clone = disk_id.clone();
    let path_clone = path.clone();

    let mut watcher = notify::recommended_watcher(
        move |result: Result<notify::Event, notify::Error>| {
            if let Ok(event) = result {
                if let Some(kind_str) = event_kind_to_string(&event.kind) {
                    let payload = FsChangeEvent {
                        disk_id: disk_id_clone.clone(),
                        path: path_clone.clone(),
                        kind: kind_str.to_string(),
                    };
                    let _ = app_clone.emit("fs-change", &payload);
                }
            }
        },
    )
    .map_err(|e| DiskDeckError::Storage(format!("Failed to create watcher: {e}")))?;

    watcher
        .watch(&full_path, RecursiveMode::NonRecursive)
        .map_err(|e| DiskDeckError::Storage(format!("Failed to watch directory: {e}")))?;

    state.watchers.insert(watch_id.clone(), watcher).await;

    Ok(watch_id)
}

/// Stops watching a directory by removing its watcher.
///
/// The watcher is dropped, which stops the OS-level monitor. If the
/// watch ID is not found (already unwatched or invalid), returns an error.
#[tauri::command]
pub async fn unwatch_directory(
    state: State<'_, AppState>,
    watch_id: String,
) -> Result<(), DiskDeckError> {
    if state.watchers.remove(&watch_id).await {
        Ok(())
    } else {
        Err(DiskDeckError::NotFound(format!(
            "Watch '{}' not found",
            watch_id
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::disk::{DiskConfig, DiskType};

    /// Helper: creates an AppState with a local disk config.
    async fn state_with_local_disk(disk_id: &str, root: &str) -> AppState {
        let store = crate::db::DiskStore::new_in_memory().unwrap();
        let state = AppState::new(store);
        let disk = DiskConfig {
            id: disk_id.to_string(),
            name: "Test Local".to_string(),
            disk_type: DiskType::Local,
            config: serde_json::json!({ "root": root }),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        state.disks.write().await.push(disk);
        state
    }

    /// Helper: creates an AppState with a non-local (S3) disk config.
    async fn state_with_s3_disk(disk_id: &str) -> AppState {
        let store = crate::db::DiskStore::new_in_memory().unwrap();
        let state = AppState::new(store);
        let disk = DiskConfig {
            id: disk_id.to_string(),
            name: "Test S3".to_string(),
            disk_type: DiskType::S3,
            config: serde_json::json!({ "bucket": "test" }),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        state.disks.write().await.push(disk);
        state
    }

    #[tokio::test]
    async fn watch_returns_error_for_non_local_disk() {
        let state = state_with_s3_disk("s3-disk").await;
        let result = resolve_local_path(&state, "s3-disk", "/").await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("only supported for local"));
    }

    #[tokio::test]
    async fn watch_returns_error_for_unknown_disk() {
        let state = state_with_local_disk("d1", "/tmp").await;
        let result = resolve_local_path(&state, "nonexistent", "/").await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("not found"));
    }

    #[tokio::test]
    async fn resolve_local_path_succeeds_for_local_disk() {
        let state = state_with_local_disk("d1", "/tmp/test-root").await;
        let result = resolve_local_path(&state, "d1", "/subdir").await;
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.ends_with("subdir"));
    }

    #[tokio::test]
    async fn unwatch_returns_error_for_unknown_id() {
        let store = crate::db::DiskStore::new_in_memory().unwrap();
        let state = AppState::new(store);
        let result = state.watchers.remove("nonexistent").await;
        assert!(!result);
    }

    #[test]
    fn event_kind_mapping() {
        assert_eq!(
            event_kind_to_string(&EventKind::Create(notify::event::CreateKind::File)),
            Some("create")
        );
        assert_eq!(
            event_kind_to_string(&EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Content
            ))),
            Some("modify")
        );
        assert_eq!(
            event_kind_to_string(&EventKind::Remove(notify::event::RemoveKind::File)),
            Some("delete")
        );
        assert_eq!(
            event_kind_to_string(&EventKind::Access(notify::event::AccessKind::Read)),
            None
        );
    }
}
