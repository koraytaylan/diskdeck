//! # Filesystem watcher for live directory updates
//!
//! Provides Tauri IPC commands to start and stop watching local directories
//! for changes. When a file is created, modified, or deleted in a watched
//! directory, a **debounced/coalesced** `"fs-change"` event is emitted to the
//! frontend so the file list can auto-refresh.
//!
//! ## Debouncing / Coalescing
//!
//! High-frequency events for the same `(disk, path)` watch are coalesced:
//! only a single `"fs-change"` notification is delivered after
//! `DEBOUNCE_DURATION_MS` (200 ms) of silence. This dramatically reduces
//! IPC noise for busy directories (builds, node_modules, Downloads, etc.)
//! while preserving the exact same `FsChangeEvent` payload shape.
//! The frontend listener in `FileContext` requires zero changes.
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
//!
//! Rapid events are coalesced inside the watcher module using per-watch-ID
//! `tokio::task::JoinHandle` timers (cancelled cleanly on unwatch or drop).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;
use tokio::{runtime::Handle as TokioHandle, task::JoinHandle, time::sleep};

use crate::error::DiskDeckError;
use crate::models::disk::DiskType;
use crate::state::AppState;

/// Debounce window for coalescing rapid fs-change events into a single notification.
/// Chosen to balance responsiveness (UI feels live) with noise reduction under churn.
const DEBOUNCE_DURATION_MS: u64 = 200;

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
///
/// Also manages per-watch-ID debounce timers to coalesce high-frequency
/// fs-change events (see `DEBOUNCE_DURATION_MS` and `schedule_debounced`).
pub struct WatcherRegistry {
    /// Map of watch ID to active watcher instance.
    watchers: RwLock<HashMap<String, RecommendedWatcher>>,
    /// Shared state for pending debounce timers (one per active watch ID).
    /// Uses std::sync::Mutex because it is accessed from both async commands
    /// and the synchronous notify callback closure (which may run on a
    /// background OS thread).
    pending: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
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
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Inserts a watcher into the registry.
    pub async fn insert(&self, id: String, watcher: RecommendedWatcher) {
        self.watchers.write().await.insert(id, watcher);
    }

    /// Removes and drops a watcher, stopping filesystem monitoring.
    ///
    /// Also aborts any pending debounce timer for that watch ID (prevents
    /// a late emission after the directory is no longer watched).
    ///
    /// Returns `true` if the watcher was found and removed, `false` otherwise.
    pub async fn remove(&self, id: &str) -> bool {
        if let Ok(mut pending) = self.pending.lock() {
            if let Some(handle) = pending.remove(id) {
                handle.abort();
            }
        }
        self.watchers.write().await.remove(id).is_some()
    }
}

impl Drop for WatcherRegistry {
    /// On registry drop (e.g. app shutdown), abort all pending debounce tasks
    /// to avoid orphaned timers.
    fn drop(&mut self) {
        if let Ok(mut pending) = self.pending.lock() {
            for (_, handle) in pending.drain() {
                handle.abort();
            }
        }
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

/// Schedules (or reschedules) a debounced filesystem change emission for a
/// given watch ID.
///
/// This is the core of the coalescing logic: every incoming fs event for a
/// watch cancels any prior pending timer and arms a fresh one. When the
/// timer fires after `duration` of silence, the provided `emitter` closure
/// is invoked exactly once (the "coalesced" notification).
///
/// The function is synchronous so it can be called directly from the
/// `notify` callback closure (which runs on a non-async OS thread).
fn schedule_debounced<F>(
    pending: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    duration: Duration,
    watch_id: String,
    emitter: F,
    rt_handle: TokioHandle,
) where
    F: FnOnce() + Send + 'static,
{
    {
        let mut timers = pending.lock().unwrap();
        if let Some(h) = timers.remove(&watch_id) {
            h.abort();
        }

        let pending_task = pending.clone();
        let wid = watch_id.clone();

        let handle = rt_handle.spawn(async move {
            sleep(duration).await;
            emitter();
            if let Ok(mut t) = pending_task.lock() {
                t.remove(&wid);
            }
        });

        timers.insert(watch_id, handle);
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
/// directory (non-recursively). Incoming events are debounced/coalesced
/// per watch ID (see `DEBOUNCE_DURATION_MS`) so that the frontend receives
/// at most one `"fs-change"` notification after a quiet period, even under
/// very high event rates (e.g. builds, installs, log churn).
///
/// The public event payload shape is unchanged; callers of the command and
/// the frontend listener require no modifications.
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

    // Capture everything needed by the sync notify callback and the debouncer.
    // The callback may execute on a non-tokio OS thread, so we pass a runtime
    // Handle (obtained while we are still on the async command thread) and
    // the Arc-protected pending map (cheap to clone).
    let rt_handle = TokioHandle::current();
    let pending_arc = state.watchers.pending.clone();
    let closure_watch_id = watch_id.clone();
    let closure_disk = disk_id_clone.clone();
    let closure_path = path_clone.clone();
    let closure_app = app_clone.clone();
    let closure_rt = rt_handle.clone();

    let mut watcher = notify::recommended_watcher(
        move |result: Result<notify::Event, notify::Error>| {
            if let Ok(event) = result {
                if let Some(_kind_str) = event_kind_to_string(&event.kind) {
                    // Coalesce: every event for this watch cancels the prior timer
                    // and schedules a fresh one. Only the final emitter (after
                    // silence) actually emits the single "fs-change" event.
                    let app_for_emit = closure_app.clone();
                    let disk_for_emit = closure_disk.clone();
                    let path_for_emit = closure_path.clone();
                    let emitter = move || {
                        let payload = FsChangeEvent {
                            disk_id: disk_for_emit,
                            path: path_for_emit,
                            kind: "modify".to_string(),
                        };
                        let _ = app_for_emit.emit("fs-change", &payload);
                    };

                    schedule_debounced(
                        pending_arc.clone(),
                        Duration::from_millis(DEBOUNCE_DURATION_MS),
                        closure_watch_id.clone(),
                        emitter,
                        closure_rt.clone(),
                    );
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
/// The watcher is dropped, which stops the OS-level monitor. Any pending
/// debounce timer for the watch ID is aborted (no stray "fs-change" will
/// be emitted after unwatch). If the watch ID is not found, returns an error.
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

    // ─── Debounce / coalescing tests (for Issue #4) ────────────────────────

    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Core debounce logic test: N rapid simulated events must produce
    /// exactly 1 emission after the quiet window (coalescing).
    #[tokio::test]
    async fn debounce_coalesces_many_rapid_events_into_one() {
        let pending: Arc<Mutex<HashMap<String, JoinHandle<()>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let duration = Duration::from_millis(40); // short window for fast test
        let emit_count = Arc::new(AtomicUsize::new(0));
        let rt = TokioHandle::current();
        let watch_id = "test-watch-debounce".to_string();

        // Fire 8 rapid "fs events" with tiny gaps << debounce window
        for _ in 0..8 {
            let count = emit_count.clone();
            let emitter = move || {
                count.fetch_add(1, Ordering::SeqCst);
            };
            schedule_debounced(
                pending.clone(),
                duration,
                watch_id.clone(),
                emitter,
                rt.clone(),
            );
            tokio::time::sleep(Duration::from_millis(3)).await;
        }

        // Allow the final timer to fire (window + margin)
        tokio::time::sleep(Duration::from_millis(80)).await;

        assert_eq!(
            emit_count.load(Ordering::SeqCst),
            1,
            "8 rapid events must coalesce to exactly one emission"
        );

        // Pending map must be empty after successful fire + cleanup
        let p = pending.lock().unwrap();
        assert!(
            p.is_empty(),
            "pending timer entry must be removed after emission"
        );
    }

    /// Cancelling (unwatch) must prevent a pending timer from ever emitting.
    #[tokio::test]
    async fn debounce_timer_is_aborted_on_cancel() {
        let pending: Arc<Mutex<HashMap<String, JoinHandle<()>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let duration = Duration::from_millis(150);
        let emit_count = Arc::new(AtomicUsize::new(0));
        let rt = TokioHandle::current();
        let watch_id = "test-watch-cancel".to_string();

        let count = emit_count.clone();
        let emitter = move || {
            count.fetch_add(1, Ordering::SeqCst);
        };
        schedule_debounced(
            pending.clone(),
            duration,
            watch_id.clone(),
            emitter,
            rt.clone(),
        );

        // Simulate immediate unwatch / cancel
        {
            let mut p = pending.lock().unwrap();
            if let Some(h) = p.remove(&watch_id) {
                h.abort();
            }
        }

        // Wait past the would-be window
        tokio::time::sleep(Duration::from_millis(220)).await;

        assert_eq!(
            emit_count.load(Ordering::SeqCst),
            0,
            "aborted timer must never call the emitter"
        );
    }
}
