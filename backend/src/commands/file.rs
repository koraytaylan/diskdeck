//! # File operation commands
//!
//! Implements Tauri IPC commands for all file and directory operations:
//! listing, reading, writing, copying, moving, deleting, renaming, and
//! creating folders.
//!
//! ## Background job model
//!
//! Bulk operations (copy, move, delete) are now **non-blocking**: the command
//! handler creates a job in the [`JobRegistry`](crate::state::JobRegistry),
//! spawns an async task to do the actual work, and immediately returns the
//! job ID to the frontend.
//!
//! The spawned task:
//! 1. Processes entries one by one.
//! 2. Updates progress in the registry and emits `"job-update"` events.
//! 3. Checks the job's `cancel_flag` between items for cooperative cancellation.
//! 4. Transitions the job to a terminal state when done.
//!
//! ## Cancellation
//!
//! Cancellation is **cooperative**: the spawned task checks an `AtomicBool`
//! flag between files. A cancelled job:
//! - Sets its status to `Cancelled` in the registry.
//! - Emits a final `"job-update"` event.
//! - May leave the operation partially completed (some files copied/moved,
//!   others not). The frontend should handle this gracefully.
//!
//! ## Index maintenance
//!
//! File mutations (write, delete, rename, copy, move, create folder) update
//! the FTS5 search index incrementally where possible:
//! - `write_file` indexes the new file.
//! - `delete_entries` removes entries and their children from the index.
//! - `rename_entry` removes the old entry and indexes the new one; marks the
//!   disk index as stale if the entry was a directory (child paths change).
//! - `copy_entries` and `move_entries` mark the index as stale because they
//!   may involve directory trees with many children.
//!
//! Index errors are logged but never fail the file operation itself.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::DiskDeckError;
use crate::models::entry::{Entry, SizeEntry};
use crate::models::job::{JobInfo, JobKind, JobStatus};
use crate::state::{AppState, JobRegistry};

/// Retrieves the storage backend for a given disk ID from the shared state.
///
/// # Errors
///
/// Retrieves the storage backend for a disk, constructing it lazily on first access.
///
/// If the backend is already in the registry, returns it immediately.
/// Otherwise, looks up the disk config, builds the backend, caches it,
/// and returns it. Credentials are read directly from the config stored
/// in the encrypted database — no OS keychain access is needed here.
/// Cache of recent backend construction failures.
/// Prevents concurrent callers from retrying a failed connection immediately.
/// Entries expire after 30 seconds so the user can retry via the Retry button.
///
/// Uses `std::sync::Mutex` (not `tokio::sync::Mutex`) intentionally because
/// the lock is never held across `.await` points — each critical section just
/// reads or writes the HashMap and immediately drops the guard.
static BACKEND_FAILURES: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, (String, std::time::Instant)>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// How long a failed backend construction is cached before allowing retry.
const FAILURE_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(5);

pub(crate) async fn get_backend(
    state: &AppState,
    disk_id: &str,
) -> Result<std::sync::Arc<dyn crate::storage::StorageBackend>, DiskDeckError> {
    // Fast path: backend already constructed
    {
        let backends = state.backends.read().await;
        if let Some(backend) = backends.get(disk_id) {
            return Ok(backend.clone());
        }
    }

    // Check if this disk recently failed — return the cached error instead
    // of retrying immediately. The user must click Retry (which waits for
    // the TTL to expire) to try again.
    {
        let failures = BACKEND_FAILURES.lock().unwrap();
        if let Some((error_msg, when)) = failures.get(disk_id) {
            if when.elapsed() < FAILURE_CACHE_TTL {
                return Err(DiskDeckError::Storage(error_msg.clone()));
            }
        }
    }

    // Slow path: acquire a per-disk lock to ensure only one construction
    // runs at a time. Concurrent callers for the same disk wait here and
    // then find the backend already constructed (or the failure cached).
    let disk_lock = {
        let mut locks = state.backend_locks.lock().await;
        locks.entry(disk_id.to_string())
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    let _guard = disk_lock.lock().await;

    // Re-check after acquiring the per-disk lock
    {
        let backends = state.backends.read().await;
        if let Some(backend) = backends.get(disk_id) {
            return Ok(backend.clone());
        }
    }
    // Also re-check failures (first caller may have just failed)
    {
        let failures = BACKEND_FAILURES.lock().unwrap();
        if let Some((error_msg, when)) = failures.get(disk_id) {
            if when.elapsed() < FAILURE_CACHE_TTL {
                return Err(DiskDeckError::Storage(error_msg.clone()));
            }
        }
    }

    let disk = {
        let disks = state.disks.read().await;
        disks.iter().find(|d| d.id == disk_id).cloned()
    };
    let disk = disk.ok_or_else(|| DiskDeckError::NotFound(format!("Disk '{}' not found", disk_id)))?;

    match crate::commands::disk::backend_from_config(&disk).await {
        Ok(backend) => {
            // Clear any previous failure
            BACKEND_FAILURES.lock().unwrap().remove(disk_id);
            state.backends.write().await.insert(disk_id.to_string(), backend.clone());
            Ok(backend)
        }
        Err(e) => {
            // Cache the failure so concurrent/subsequent callers don't retry
            let msg = e.to_string();
            BACKEND_FAILURES.lock().unwrap().insert(
                disk_id.to_string(),
                (msg.clone(), std::time::Instant::now()),
            );
            Err(e)
        }
    }
}

/// Marks a disk's search index as stale so the background indexer will
/// re-walk it on the next run. Best-effort: logs errors silently.
fn mark_stale(state: &AppState, disk_id: &str) {
    if let Err(e) = state.store.set_index_meta(disk_id, "stale", 0) {
        log::warn!("Index mark stale error: {e}");
    }
}

/// Emits the current job state to the frontend as a `"job-update"` event.
///
/// Best-effort: if the event cannot be emitted (e.g., the frontend window
/// is closed), the error is silently ignored. This ensures progress reporting
/// never causes the underlying file operation to fail.
async fn emit_job_update(app: &AppHandle, registry: &Arc<JobRegistry>, job_id: &str) {
    if let Some(info) = registry.get(job_id).await {
        let _ = app.emit("job-update", &info);
    }
}

/// Builds a human-readable job description from the source paths and destination.
///
/// Examples: `"readme.md → /Documents"`, `"3 files → /backup"`, `"2 files"` (for delete).
fn job_description(paths: &[String], dest: Option<&str>) -> String {
    let names: String = if paths.len() == 1 {
        std::path::Path::new(&paths[0])
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| paths[0].clone())
    } else {
        format!("{} files", paths.len())
    };
    match dest {
        Some(d) => format!("{names} → {d}"),
        None => names,
    }
}

/// Recursively copies a single entry from one backend to another.
///
/// For files: reads the full byte content from the source backend and writes
/// it to the destination backend.
/// For directories: creates the directory on the destination, lists all
/// children from the source, and recurses into each child.
///
/// Uses `Box::pin` for the recursive async calls to satisfy the compiler's
/// requirement for a known future size.
async fn cross_copy_single(
    src: &std::sync::Arc<dyn crate::storage::StorageBackend>,
    dst: &std::sync::Arc<dyn crate::storage::StorageBackend>,
    src_path: &str,
    dst_path: &str,
) -> Result<(), DiskDeckError> {
    let stat = src.stat(src_path).await?;
    if stat.is_dir {
        dst.create_dir(dst_path).await?;
        let children = src.list(src_path).await?;
        for child in children {
            let child_dst = format!("{}/{}", dst_path.trim_end_matches('/'), child.name);
            // Use Box::pin for recursive async
            Box::pin(cross_copy_single(src, dst, &child.path, &child_dst)).await?;
        }
    } else {
        let data = src.read(src_path).await?;
        dst.write(dst_path, &data).await?;
    }
    Ok(())
}

/// Builds a human-readable job description for cross-disk transfers.
///
/// Includes the destination disk name to distinguish from same-disk operations.
/// Examples: `"readme.md → S3 Backup:/archive"`, `"3 files → NAS:/backup"`.
fn cross_disk_description(paths: &[String], dst_disk_name: &str, dest: &str) -> String {
    let names: String = if paths.len() == 1 {
        std::path::Path::new(&paths[0])
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| paths[0].clone())
    } else {
        format!("{} files", paths.len())
    };
    format!("{names} → {dst_disk_name}:{dest}")
}

/// Lists entries (files and directories) at the given path on a disk.
#[tauri::command]
pub async fn list_entries(
    state: State<'_, AppState>,
    disk_id: String,
    path: String,
) -> Result<Vec<Entry>, DiskDeckError> {
    let backend = get_backend(&state, &disk_id).await?;
    backend.list(&path).await
}

/// Returns metadata for a single entry (file or directory).
#[tauri::command]
pub async fn get_entry(
    state: State<'_, AppState>,
    disk_id: String,
    path: String,
) -> Result<Entry, DiskDeckError> {
    let backend = get_backend(&state, &disk_id).await?;
    backend.stat(&path).await
}

/// Reads the full contents of a file and returns it as a byte array.
///
/// The entire file is loaded into memory. For large files, the frontend
/// should consider streaming or pagination (not yet implemented).
#[tauri::command]
pub async fn read_file(
    state: State<'_, AppState>,
    disk_id: String,
    path: String,
) -> Result<Vec<u8>, DiskDeckError> {
    let backend = get_backend(&state, &disk_id).await?;
    backend.read(&path).await
}

/// Core logic for writing a file and indexing it, testable without Tauri State.
pub(crate) async fn write_file_inner(
    state: &AppState,
    disk_id: &str,
    path: &str,
    data: &[u8],
) -> Result<(), DiskDeckError> {
    let backend = get_backend(state, disk_id).await?;
    backend.write(path, data).await?;
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Err(e) = state.store.index_entry(disk_id, path, &name, false, data.len() as u64, None) {
        log::warn!("Failed to index written file: {e}");
    }
    Ok(())
}

/// Writes data to a file and indexes it in the search index.
///
/// Creates the file if it does not exist, or overwrites if it does.
/// The new file is immediately added to the FTS5 search index.
#[tauri::command]
pub async fn write_file(
    state: State<'_, AppState>,
    disk_id: String,
    path: String,
    data: Vec<u8>,
) -> Result<(), DiskDeckError> {
    write_file_inner(&state, &disk_id, &path, &data).await
}

/// Sets up a copy job: resolves backend, creates the job in the registry.
/// Returns the job ID, cancellation flag, and backend for the spawned task.
#[cfg(test)]
pub(crate) async fn setup_copy_job(
    state: &AppState,
    disk_id: &str,
    paths: &[String],
    dest: &str,
) -> Result<(String, std::sync::Arc<std::sync::atomic::AtomicBool>, std::sync::Arc<dyn crate::storage::StorageBackend>), DiskDeckError> {
    let backend = get_backend(state, disk_id).await?;
    let total = paths.len() as u32;
    let desc = job_description(paths, Some(dest));
    let (job_id, cancel_flag) = state.jobs.create(JobKind::Copy, total, desc, disk_id.to_string(), dest.to_string()).await;
    Ok((job_id, cancel_flag, backend))
}

/// Runs the copy loop: iterates over source paths, copies each to dest.
/// Updates progress and checks cancellation between items.
/// Returns the terminal job status after all items are processed.
pub(crate) async fn run_copy_loop(
    registry: &Arc<JobRegistry>,
    job_id: &str,
    cancel_flag: &std::sync::atomic::AtomicBool,
    backend: &dyn crate::storage::StorageBackend,
    paths: &[String],
    dest: &str,
) -> JobStatus {
    for (i, src) in paths.iter().enumerate() {
        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
            registry.finish(job_id, JobStatus::Cancelled, None).await;
            return JobStatus::Cancelled;
        }
        registry.update_progress(job_id, i as u32, src).await;
        let file_name = std::path::Path::new(src.as_str())
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dst = format!("{}/{}", dest.trim_end_matches('/'), file_name);
        if let Err(e) = backend.copy(src, &dst).await {
            registry.finish(job_id, JobStatus::Failed, Some(e.to_string())).await;
            return JobStatus::Failed;
        }
    }
    registry.finish(job_id, JobStatus::Completed, None).await;
    JobStatus::Completed
}

/// Runs the move loop: iterates over source paths, renames each to dest.
pub(crate) async fn run_move_loop(
    registry: &Arc<JobRegistry>,
    job_id: &str,
    cancel_flag: &std::sync::atomic::AtomicBool,
    backend: &dyn crate::storage::StorageBackend,
    paths: &[String],
    dest: &str,
) -> JobStatus {
    for (i, src) in paths.iter().enumerate() {
        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
            registry.finish(job_id, JobStatus::Cancelled, None).await;
            return JobStatus::Cancelled;
        }
        registry.update_progress(job_id, i as u32, src).await;
        let file_name = std::path::Path::new(src.as_str())
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dst = format!("{}/{}", dest.trim_end_matches('/'), file_name);
        if let Err(e) = backend.rename(src, &dst).await {
            registry.finish(job_id, JobStatus::Failed, Some(e.to_string())).await;
            return JobStatus::Failed;
        }
    }
    registry.finish(job_id, JobStatus::Completed, None).await;
    JobStatus::Completed
}

/// Runs the delete loop: iterates over paths, deletes each entry and
/// removes it from the search index.
pub(crate) async fn run_delete_loop(
    registry: &Arc<JobRegistry>,
    job_id: &str,
    cancel_flag: &std::sync::atomic::AtomicBool,
    backend: &dyn crate::storage::StorageBackend,
    store: &crate::db::DiskStore,
    disk_id: &str,
    paths: &[String],
) -> JobStatus {
    for (i, path) in paths.iter().enumerate() {
        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
            registry.finish(job_id, JobStatus::Cancelled, None).await;
            return JobStatus::Cancelled;
        }
        registry.update_progress(job_id, i as u32, path).await;
        if let Err(e) = backend.delete(path).await {
            registry.finish(job_id, JobStatus::Failed, Some(e.to_string())).await;
            return JobStatus::Failed;
        }
        if let Err(e) = store.remove_entry(disk_id, path) {
            log::warn!("Failed to remove index entry: {e}");
        }
        if let Err(e) = store.remove_entries_under(disk_id, path) {
            log::warn!("Failed to remove index entries under path: {e}");
        }
    }
    registry.finish(job_id, JobStatus::Completed, None).await;
    JobStatus::Completed
}

/// Runs the cross-copy loop: copies entries from one backend to another.
pub(crate) async fn run_cross_copy_loop(
    registry: &Arc<JobRegistry>,
    job_id: &str,
    cancel_flag: &std::sync::atomic::AtomicBool,
    src_backend: &Arc<dyn crate::storage::StorageBackend>,
    dst_backend: &Arc<dyn crate::storage::StorageBackend>,
    paths: &[String],
    dest: &str,
) -> JobStatus {
    for (i, src_path) in paths.iter().enumerate() {
        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
            registry.finish(job_id, JobStatus::Cancelled, None).await;
            return JobStatus::Cancelled;
        }
        registry.update_progress(job_id, i as u32, src_path).await;
        let file_name = std::path::Path::new(src_path.as_str())
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dst_path = format!("{}/{}", dest.trim_end_matches('/'), file_name);
        if let Err(e) = cross_copy_single(src_backend, dst_backend, src_path, &dst_path).await {
            registry.finish(job_id, JobStatus::Failed, Some(e.to_string())).await;
            return JobStatus::Failed;
        }
    }
    registry.finish(job_id, JobStatus::Completed, None).await;
    JobStatus::Completed
}

/// Runs the cross-move loop: copies each entry then deletes from source.
pub(crate) async fn run_cross_move_loop(
    registry: &Arc<JobRegistry>,
    job_id: &str,
    cancel_flag: &std::sync::atomic::AtomicBool,
    src_backend: &Arc<dyn crate::storage::StorageBackend>,
    dst_backend: &Arc<dyn crate::storage::StorageBackend>,
    paths: &[String],
    dest: &str,
) -> JobStatus {
    for (i, src_path) in paths.iter().enumerate() {
        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
            registry.finish(job_id, JobStatus::Cancelled, None).await;
            return JobStatus::Cancelled;
        }
        registry.update_progress(job_id, i as u32, src_path).await;
        let file_name = std::path::Path::new(src_path.as_str())
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dst_path = format!("{}/{}", dest.trim_end_matches('/'), file_name);
        if let Err(e) = cross_copy_single(src_backend, dst_backend, src_path, &dst_path).await {
            registry.finish(job_id, JobStatus::Failed, Some(e.to_string())).await;
            return JobStatus::Failed;
        }
        if let Err(e) = src_backend.delete(src_path).await {
            registry.finish(job_id, JobStatus::Failed, Some(e.to_string())).await;
            return JobStatus::Failed;
        }
    }
    registry.finish(job_id, JobStatus::Completed, None).await;
    JobStatus::Completed
}

/// Copies multiple entries to a destination directory as a background job.
///
/// Each source path is copied to `dest/<filename>`. Returns the job ID
/// immediately while the actual copying proceeds in a spawned async task.
///
/// Marks the disk's search index as stale after completion because copied
/// directory trees may contain many new entries.
///
/// # Returns
///
/// The job ID (UUID string) for progress tracking and cancellation.
#[tauri::command]
pub async fn copy_entries(
    app: AppHandle,
    state: State<'_, AppState>,
    disk_id: String,
    paths: Vec<String>,
    dest: String,
) -> Result<String, DiskDeckError> {
    let backend = get_backend(&state, &disk_id).await?;
    let total = paths.len() as u32;
    let desc = job_description(&paths, Some(&dest));
    let (job_id, cancel_flag) = state.jobs.create(JobKind::Copy, total, desc, disk_id.clone(), dest.clone()).await;
    let registry = state.jobs.clone();

    // Emit initial state so the frontend can show the job immediately
    emit_job_update(&app, &registry, &job_id).await;

    let app2 = app.clone();
    let jid = job_id.clone();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        run_copy_loop(&registry, &jid, &cancel_flag, backend.as_ref(), &paths, &dest).await;
        emit_job_update(&app2, &registry, &jid).await;

        // Mark disk index as stale for reindexing
        let state = app_handle.state::<crate::state::AppState>();
        mark_stale(&state, &disk_id);
    });

    Ok(job_id)
}

/// Moves multiple entries to a destination directory as a background job.
///
/// Each source path is renamed to `dest/<filename>`. Uses the backend's
/// `rename` method, which is atomic on local filesystems but may be
/// copy-then-delete on remote backends.
///
/// # Returns
///
/// The job ID (UUID string) for progress tracking and cancellation.
#[tauri::command]
pub async fn move_entries(
    app: AppHandle,
    state: State<'_, AppState>,
    disk_id: String,
    paths: Vec<String>,
    dest: String,
) -> Result<String, DiskDeckError> {
    let backend = get_backend(&state, &disk_id).await?;
    let total = paths.len() as u32;
    let desc = job_description(&paths, Some(&dest));
    let (job_id, cancel_flag) = state.jobs.create(JobKind::Move, total, desc, disk_id.clone(), dest.clone()).await;
    let registry = state.jobs.clone();

    // Emit initial state
    emit_job_update(&app, &registry, &job_id).await;

    let app2 = app.clone();
    let jid = job_id.clone();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        run_move_loop(&registry, &jid, &cancel_flag, backend.as_ref(), &paths, &dest).await;
        emit_job_update(&app2, &registry, &jid).await;

        // Move may involve directory trees — mark stale for reindex
        let state = app_handle.state::<crate::state::AppState>();
        mark_stale(&state, &disk_id);
    });

    Ok(job_id)
}

/// Copies entries from one disk to another as a background job.
///
/// Reads from the source backend and writes to the destination backend.
/// For directories, copies recursively (creates dirs + copies files).
/// Returns the job ID immediately while the actual copying proceeds in a
/// spawned async task.
///
/// # Returns
///
/// The job ID (UUID string) for progress tracking and cancellation.
#[tauri::command]
pub async fn cross_copy_entries(
    app: AppHandle,
    state: State<'_, AppState>,
    src_disk_id: String,
    dst_disk_id: String,
    paths: Vec<String>,
    dest: String,
) -> Result<String, DiskDeckError> {
    let src_backend = get_backend(&state, &src_disk_id).await?;
    let dst_backend = get_backend(&state, &dst_disk_id).await?;
    let total = paths.len() as u32;

    // Look up destination disk name for the job description
    let dst_disk_name = {
        let disks = state.disks.read().await;
        disks
            .iter()
            .find(|d| d.id == dst_disk_id)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| dst_disk_id.clone())
    };
    let desc = cross_disk_description(&paths, &dst_disk_name, &dest);
    let (job_id, cancel_flag) = state.jobs.create(JobKind::Copy, total, desc, dst_disk_id.clone(), dest.clone()).await;
    let registry = state.jobs.clone();

    emit_job_update(&app, &registry, &job_id).await;

    let app2 = app.clone();
    let jid = job_id.clone();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        run_cross_copy_loop(&registry, &jid, &cancel_flag, &src_backend, &dst_backend, &paths, &dest).await;
        emit_job_update(&app2, &registry, &jid).await;

        // Mark destination disk's index as stale (new files added)
        let state = app_handle.state::<crate::state::AppState>();
        mark_stale(&state, &dst_disk_id);
    });

    Ok(job_id)
}

/// Moves entries from one disk to another (cross-copy then delete source).
///
/// Each entry is fully copied to the destination backend before the source
/// is deleted, ensuring no data loss if the operation is interrupted.
/// Returns the job ID immediately while the actual transfer proceeds in a
/// spawned async task.
///
/// # Returns
///
/// The job ID (UUID string) for progress tracking and cancellation.
#[tauri::command]
pub async fn cross_move_entries(
    app: AppHandle,
    state: State<'_, AppState>,
    src_disk_id: String,
    dst_disk_id: String,
    paths: Vec<String>,
    dest: String,
) -> Result<String, DiskDeckError> {
    let src_backend = get_backend(&state, &src_disk_id).await?;
    let dst_backend = get_backend(&state, &dst_disk_id).await?;
    let total = paths.len() as u32;

    let dst_disk_name = {
        let disks = state.disks.read().await;
        disks
            .iter()
            .find(|d| d.id == dst_disk_id)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| dst_disk_id.clone())
    };
    let desc = cross_disk_description(&paths, &dst_disk_name, &dest);
    let (job_id, cancel_flag) = state.jobs.create(JobKind::Move, total, desc, dst_disk_id.clone(), dest.clone()).await;
    let registry = state.jobs.clone();

    emit_job_update(&app, &registry, &job_id).await;

    let app2 = app.clone();
    let jid = job_id.clone();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        run_cross_move_loop(&registry, &jid, &cancel_flag, &src_backend, &dst_backend, &paths, &dest).await;
        emit_job_update(&app2, &registry, &jid).await;

        // Mark both disks' indexes as stale
        let state = app_handle.state::<crate::state::AppState>();
        mark_stale(&state, &src_disk_id);
        mark_stale(&state, &dst_disk_id);
    });

    Ok(job_id)
}

/// Deletes multiple entries (files and/or directories) as a background job.
///
/// Each deleted entry is also removed from the FTS5 search index,
/// including all child entries under the path (for directory deletions).
///
/// # Returns
///
/// The job ID (UUID string) for progress tracking and cancellation.
#[tauri::command]
pub async fn delete_entries(
    app: AppHandle,
    state: State<'_, AppState>,
    disk_id: String,
    paths: Vec<String>,
) -> Result<String, DiskDeckError> {
    let backend = get_backend(&state, &disk_id).await?;
    let total = paths.len() as u32;
    let desc = job_description(&paths, None);
    // Target path is the parent directory of the deleted entries
    let target_path = paths.first()
        .and_then(|p| p.rsplit_once('/').map(|(parent, _)| if parent.is_empty() { "/" } else { parent }))
        .unwrap_or("/")
        .to_string();
    let (job_id, cancel_flag) = state.jobs.create(JobKind::Delete, total, desc, disk_id.clone(), target_path).await;
    let registry = state.jobs.clone();

    // Emit initial state
    emit_job_update(&app, &registry, &job_id).await;

    let app2 = app.clone();
    let jid = job_id.clone();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<crate::state::AppState>();
        run_delete_loop(&registry, &jid, &cancel_flag, backend.as_ref(), &state.store, &disk_id, &paths).await;
        emit_job_update(&app2, &registry, &jid).await;
    });

    Ok(job_id)
}

/// Core logic for renaming an entry, testable without Tauri State.
pub(crate) async fn rename_entry_inner(
    state: &AppState,
    disk_id: &str,
    path: &str,
    new_name: &str,
) -> Result<(), DiskDeckError> {
    // Validate new_name: reject path separators and traversal
    if new_name.contains('/') || new_name.contains('\\') || new_name == "." || new_name == ".." {
        return Err(DiskDeckError::Storage("Invalid file name".into()));
    }
    if new_name.is_empty() {
        return Err(DiskDeckError::Storage("File name cannot be empty".into()));
    }

    let backend = get_backend(state, disk_id).await?;
    let parent = std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".to_string());
    let new_path = format!("{}/{}", parent.trim_end_matches('/'), new_name);
    backend.rename(path, &new_path).await?;

    // Update index: remove old entry and children, add the new entry
    if let Err(e) = state.store.remove_entry(disk_id, path) {
        log::warn!("Failed to remove old index entry on rename: {e}");
    }
    if let Err(e) = state.store.remove_entries_under(disk_id, path) {
        log::warn!("Failed to remove old index entries on rename: {e}");
    }
    // Stat the new entry for accurate metadata
    if let Ok(entry) = backend.stat(&new_path).await {
        let _ = state.store.index_entry(
            disk_id,
            &new_path,
            new_name,
            entry.is_dir,
            entry.size,
            entry.modified,
        );
    }
    // If it was a directory, children paths changed — mark stale
    mark_stale(state, disk_id);
    Ok(())
}

/// Renames a single entry (file or directory).
///
/// # Validation
///
/// The new name must:
/// - Not be empty.
/// - Not contain path separators (`/`, `\`).
/// - Not be `.` or `..`.
///
/// # Index maintenance
///
/// The old entry (and children, if a directory) are removed from the index.
/// The new entry is stat'd and indexed. If the renamed item was a directory,
/// the disk is marked stale because all children's paths have changed.
#[tauri::command]
pub async fn rename_entry(
    state: State<'_, AppState>,
    disk_id: String,
    path: String,
    new_name: String,
) -> Result<(), DiskDeckError> {
    rename_entry_inner(&state, &disk_id, &path, &new_name).await
}

/// Core logic for creating a folder and indexing it, testable without Tauri State.
pub(crate) async fn create_folder_inner(
    state: &AppState,
    disk_id: &str,
    path: &str,
) -> Result<(), DiskDeckError> {
    let backend = get_backend(state, disk_id).await?;
    backend.create_dir(path).await?;
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Err(e) = state.store.index_entry(disk_id, path, &name, true, 0, None) {
        log::warn!("Failed to index new folder: {e}");
    }
    Ok(())
}

/// Creates a new empty directory and indexes it.
#[tauri::command]
pub async fn create_folder(
    state: State<'_, AppState>,
    disk_id: String,
    path: String,
) -> Result<(), DiskDeckError> {
    create_folder_inner(&state, &disk_id, &path).await
}

/// Recursively calculates the total size of a directory in bytes.
///
/// Walks the entire directory tree under the given path, summing file sizes.
/// Subdirectory entries themselves contribute zero bytes — only files count.
async fn get_folder_size_inner(
    state: &AppState,
    disk_id: &str,
    path: &str,
) -> Result<u64, DiskDeckError> {
    let backend = get_backend(state, disk_id).await?;
    calculate_dir_size(&backend, path).await
}

/// Recursive helper: sums file sizes in a directory tree.
///
/// For each entry in the directory, if it is a file its size is added to the
/// total. If it is a subdirectory, the function recurses into it. Uses
/// `Box::pin` to allow the recursive async call.
async fn calculate_dir_size(
    backend: &std::sync::Arc<dyn crate::storage::StorageBackend>,
    path: &str,
) -> Result<u64, DiskDeckError> {
    let entries = backend.list(path).await?;
    let mut total: u64 = 0;
    for entry in entries {
        if entry.is_dir {
            total += Box::pin(calculate_dir_size(backend, &entry.path)).await?;
        } else {
            total += entry.size;
        }
    }
    Ok(total)
}

/// Returns the total size of a directory in bytes (recursive).
///
/// Walks the entire directory tree and sums all file sizes. Useful for
/// displaying folder sizes in the properties panel.
#[tauri::command]
pub async fn get_folder_size(
    state: State<'_, AppState>,
    disk_id: String,
    path: String,
) -> Result<u64, DiskDeckError> {
    get_folder_size_inner(&state, &disk_id, &path).await
}

/// Returns the size of each immediate child in a directory.
///
/// For files, the reported size is the file's own size. For subdirectories,
/// the size is calculated recursively (same algorithm as `get_folder_size`).
/// Results are sorted by size descending, making them suitable for rendering
/// a treemap or stacked bar chart visualization.
pub(crate) async fn get_size_breakdown_inner(
    state: &AppState,
    disk_id: &str,
    path: &str,
) -> Result<Vec<SizeEntry>, DiskDeckError> {
    let backend = get_backend(state, disk_id).await?;
    let entries = backend.list(path).await?;
    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        let size = if entry.is_dir {
            calculate_dir_size(&backend, &entry.path).await?
        } else {
            entry.size
        };
        result.push(SizeEntry {
            path: entry.path,
            name: entry.name,
            size,
            is_dir: entry.is_dir,
        });
    }
    result.sort_by_key(|b| std::cmp::Reverse(b.size));
    Ok(result)
}

/// Returns the size breakdown of a directory's immediate children.
///
/// Each child's size is reported: files use their direct size, directories
/// use their recursive total. Results are sorted by size descending.
/// Used by the frontend's disk usage treemap visualization.
#[tauri::command]
pub async fn get_size_breakdown(
    state: State<'_, AppState>,
    disk_id: String,
    path: String,
) -> Result<Vec<SizeEntry>, DiskDeckError> {
    get_size_breakdown_inner(&state, &disk_id, &path).await
}

/// Renames multiple files by applying find/replace to their filenames.
///
/// For each path in `paths`, extracts the filename, applies `find` → `replace`,
/// and renames the file if the result differs. Files whose names do not contain
/// the find pattern are silently skipped.
///
/// # Validation
///
/// - `find` must not be empty.
/// - `replace` must not contain path separators (`/` or `\`).
///
/// # Returns
///
/// A list of `(old_path, new_path)` pairs for successfully renamed entries.
async fn batch_rename_inner(
    state: &AppState,
    disk_id: &str,
    paths: &[String],
    find: &str,
    replace: &str,
) -> Result<Vec<(String, String)>, DiskDeckError> {
    if find.is_empty() {
        return Err(DiskDeckError::Storage("Find pattern cannot be empty".into()));
    }
    if replace.contains('/') || replace.contains('\\') {
        return Err(DiskDeckError::Storage(
            "Replace pattern cannot contain path separators".into(),
        ));
    }

    let backend = get_backend(state, disk_id).await?;
    let mut renamed = Vec::new();

    for path in paths {
        let file_name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let new_name = file_name.replace(find, replace);
        if new_name == file_name || new_name.is_empty() {
            continue; // No change or invalid result
        }
        let parent = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/".to_string());
        let new_path = format!("{}/{}", parent.trim_end_matches('/'), new_name);
        backend.rename(path, &new_path).await?;
        renamed.push((path.clone(), new_path));
    }

    // Mark index as stale since many paths changed
    mark_stale(state, disk_id);
    Ok(renamed)
}

/// Renames multiple files by applying find/replace to their filenames.
///
/// Applies a simple string find/replace to the basename of each selected file.
/// Files whose names do not contain the find pattern are skipped. Returns the
/// list of `(old_path, new_path)` pairs that were successfully renamed.
#[tauri::command]
pub async fn batch_rename(
    state: State<'_, AppState>,
    disk_id: String,
    paths: Vec<String>,
    find: String,
    replace: String,
) -> Result<Vec<(String, String)>, DiskDeckError> {
    batch_rename_inner(&state, &disk_id, &paths, &find, &replace).await
}

/// Core logic for cancelling a job, testable without Tauri State.
pub(crate) async fn cancel_job_inner(
    state: &AppState,
    job_id: &str,
) -> Result<(), DiskDeckError> {
    if !state.jobs.cancel(job_id).await {
        return Err(DiskDeckError::NotFound(format!("Job '{}' not found", job_id)));
    }
    Ok(())
}

/// Requests cancellation of a running job.
///
/// Sets the job's cancellation flag so the spawned task will stop at the
/// next inter-item check point. This call returns immediately; the actual
/// cancellation is asynchronous.
///
/// # Errors
///
/// Returns `DiskDeckError::NotFound` if the job ID is not in the registry.
#[tauri::command]
pub async fn cancel_job(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<(), DiskDeckError> {
    cancel_job_inner(&state, &job_id).await
}

/// Returns snapshots of all jobs, ordered by creation time (newest first).
///
/// Includes running, completed, failed, and cancelled jobs. The frontend
/// uses this to populate the job list UI on initial load or reconnect.
#[tauri::command]
pub async fn list_jobs(
    state: State<'_, AppState>,
) -> Result<Vec<JobInfo>, DiskDeckError> {
    Ok(state.jobs.list().await)
}

/// Removes all finished jobs (completed, failed, cancelled) from the registry.
///
/// Running jobs are left untouched. Typically called when the user dismisses
/// or clears the job list in the frontend.
#[tauri::command]
pub async fn clear_finished_jobs(
    state: State<'_, AppState>,
) -> Result<(), DiskDeckError> {
    state.jobs.clear_finished().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::memory::MemoryBackend;
    use crate::storage::StorageBackend;
    use crate::db::DiskStore;

    fn test_state() -> AppState {
        AppState::new(DiskStore::new_in_memory().unwrap())
    }

    async fn test_state_with_backend(disk_id: &str) -> (AppState, std::sync::Arc<MemoryBackend>) {
        let state = test_state();
        let backend = std::sync::Arc::new(MemoryBackend::new());
        state.backends.write().await.insert(disk_id.to_string(), backend.clone());
        (state, backend)
    }

    #[tokio::test]
    async fn get_backend_returns_registered() {
        let (state, _) = test_state_with_backend("d1").await;
        let backend = get_backend(&state, "d1").await;
        assert!(backend.is_ok());
    }

    #[tokio::test]
    async fn get_backend_unknown_returns_error() {
        let state = test_state();
        let result = get_backend(&state, "nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cross_copy_single_file() {
        let src = std::sync::Arc::new(MemoryBackend::new());
        let dst = std::sync::Arc::new(MemoryBackend::new());
        src.write("/hello.txt", b"world").await.unwrap();
        let src_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = src.clone();
        let dst_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = dst.clone();
        cross_copy_single(&src_dyn, &dst_dyn, "/hello.txt", "/hello.txt")
            .await
            .unwrap();
        assert_eq!(dst.read("/hello.txt").await.unwrap(), b"world");
        assert!(src.exists("/hello.txt").await.unwrap());
    }

    #[tokio::test]
    async fn cross_copy_single_directory() {
        let src = std::sync::Arc::new(MemoryBackend::new());
        let dst = std::sync::Arc::new(MemoryBackend::new());
        src.create_dir("/docs").await.unwrap();
        src.write("/docs/a.txt", b"aaa").await.unwrap();
        src.write("/docs/b.txt", b"bbb").await.unwrap();

        let src_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = src.clone();
        let dst_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = dst.clone();
        cross_copy_single(&src_dyn, &dst_dyn, "/docs", "/docs")
            .await
            .unwrap();

        assert_eq!(dst.read("/docs/a.txt").await.unwrap(), b"aaa");
        assert_eq!(dst.read("/docs/b.txt").await.unwrap(), b"bbb");
    }

    #[tokio::test]
    async fn cross_copy_single_nonexistent_errors() {
        let src = std::sync::Arc::new(MemoryBackend::new());
        let dst = std::sync::Arc::new(MemoryBackend::new());
        let src_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = src;
        let dst_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = dst;
        let result = cross_copy_single(&src_dyn, &dst_dyn, "/nope.txt", "/nope.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mark_stale_does_not_panic() {
        let state = test_state();
        mark_stale(&state, "nonexistent");
    }

    #[test]
    fn job_description_single_file_with_dest() {
        let paths = vec!["/docs/readme.md".to_string()];
        assert_eq!(job_description(&paths, Some("/backup")), "readme.md → /backup");
    }

    #[test]
    fn job_description_multiple_files_with_dest() {
        let paths = vec!["/a.txt".to_string(), "/b.txt".to_string(), "/c.txt".to_string()];
        assert_eq!(job_description(&paths, Some("/dest")), "3 files → /dest");
    }

    #[test]
    fn job_description_no_dest() {
        let paths = vec!["/file.txt".to_string()];
        assert_eq!(job_description(&paths, None), "file.txt");
    }

    #[test]
    fn job_description_multiple_no_dest() {
        let paths = vec!["/a.txt".to_string(), "/b.txt".to_string()];
        assert_eq!(job_description(&paths, None), "2 files");
    }

    #[test]
    fn cross_disk_description_single() {
        let paths = vec!["/readme.md".to_string()];
        assert_eq!(
            cross_disk_description(&paths, "S3 Backup", "/archive"),
            "readme.md → S3 Backup:/archive"
        );
    }

    #[test]
    fn cross_disk_description_multiple() {
        let paths = vec!["/a.txt".to_string(), "/b.txt".to_string()];
        assert_eq!(
            cross_disk_description(&paths, "NAS", "/backup"),
            "2 files → NAS:/backup"
        );
    }

    // ---- write_file_inner tests ----

    #[tokio::test]
    async fn write_file_inner_writes_and_indexes() {
        let (state, backend) = test_state_with_backend("d1").await;
        write_file_inner(&state, "d1", "/hello.txt", b"world")
            .await
            .unwrap();
        // Verify data was written to backend
        assert_eq!(backend.read("/hello.txt").await.unwrap(), b"world");
        // Verify index entry was created
        let results = state.store.search_index("hello", None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "/hello.txt");
        assert!(!results[0].is_dir);
        assert_eq!(results[0].size, 5);
    }

    #[tokio::test]
    async fn write_file_inner_unknown_disk_errors() {
        let state = test_state();
        let result = write_file_inner(&state, "nope", "/f.txt", b"data").await;
        assert!(result.is_err());
    }

    // ---- rename_entry_inner tests ----

    #[tokio::test]
    async fn rename_entry_inner_renames_file() {
        let (state, backend) = test_state_with_backend("d1").await;
        backend.write("/old.txt", b"data").await.unwrap();
        // Index the old entry
        state.store.index_entry("d1", "/old.txt", "old.txt", false, 4, None).unwrap();

        rename_entry_inner(&state, "d1", "/old.txt", "new.txt")
            .await
            .unwrap();

        // Backend should have new file, not old
        assert!(backend.exists("/new.txt").await.unwrap());
        assert!(!backend.exists("/old.txt").await.unwrap());

        // Old index entry should be gone, new one present
        let old_results = state.store.search_index("old", None, 10).unwrap();
        assert!(old_results.is_empty());
        let new_results = state.store.search_index("new", None, 10).unwrap();
        assert_eq!(new_results.len(), 1);
        assert_eq!(new_results[0].path, "/new.txt");
    }

    #[tokio::test]
    async fn rename_entry_inner_rejects_empty_name() {
        let (state, _) = test_state_with_backend("d1").await;
        let result = rename_entry_inner(&state, "d1", "/f.txt", "").await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("empty"));
    }

    #[tokio::test]
    async fn rename_entry_inner_rejects_slash() {
        let (state, _) = test_state_with_backend("d1").await;
        let result = rename_entry_inner(&state, "d1", "/f.txt", "a/b").await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("Invalid"));
    }

    #[tokio::test]
    async fn rename_entry_inner_rejects_backslash() {
        let (state, _) = test_state_with_backend("d1").await;
        let result = rename_entry_inner(&state, "d1", "/f.txt", "a\\b").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rename_entry_inner_rejects_dot() {
        let (state, _) = test_state_with_backend("d1").await;
        let result = rename_entry_inner(&state, "d1", "/f.txt", ".").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rename_entry_inner_rejects_dotdot() {
        let (state, _) = test_state_with_backend("d1").await;
        let result = rename_entry_inner(&state, "d1", "/f.txt", "..").await;
        assert!(result.is_err());
    }

    // ---- create_folder_inner tests ----

    #[tokio::test]
    async fn create_folder_inner_creates_and_indexes() {
        let (state, backend) = test_state_with_backend("d1").await;
        create_folder_inner(&state, "d1", "/mydir").await.unwrap();
        // Verify directory was created
        assert!(backend.exists("/mydir").await.unwrap());
        let entry = backend.stat("/mydir").await.unwrap();
        assert!(entry.is_dir);
        // Verify index entry
        let results = state.store.search_index("mydir", None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_dir);
    }

    #[tokio::test]
    async fn create_folder_inner_unknown_disk_errors() {
        let state = test_state();
        let result = create_folder_inner(&state, "nope", "/dir").await;
        assert!(result.is_err());
    }

    // ---- setup_copy_job tests ----

    #[tokio::test]
    async fn setup_copy_job_creates_job() {
        let (state, _) = test_state_with_backend("d1").await;
        let paths = vec!["/a.txt".to_string(), "/b.txt".to_string()];
        let (job_id, cancel_flag, _backend) =
            setup_copy_job(&state, "d1", &paths, "/dest").await.unwrap();
        // Job should exist in registry
        let info = state.jobs.get(&job_id).await.unwrap();
        assert_eq!(info.total, 2);
        assert_eq!(info.status, JobStatus::Running);
        assert!(info.description.contains("/dest"));
        assert!(!cancel_flag.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[tokio::test]
    async fn setup_copy_job_unknown_disk_errors() {
        let state = test_state();
        let paths = vec!["/a.txt".to_string()];
        let result = setup_copy_job(&state, "nope", &paths, "/dest").await;
        assert!(result.is_err());
    }

    // ---- cancel_job_inner tests ----

    #[tokio::test]
    async fn cancel_job_inner_cancels_existing() {
        let state = test_state();
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Copy, 1, "test".into(), "d".into(), "/".into()).await;
        cancel_job_inner(&state, &job_id).await.unwrap();
        assert!(cancel_flag.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[tokio::test]
    async fn cancel_job_inner_unknown_errors() {
        let state = test_state();
        let result = cancel_job_inner(&state, "nonexistent").await;
        assert!(result.is_err());
    }

    // ---- run_copy_loop tests ----

    #[tokio::test]
    async fn run_copy_loop_copies_files() {
        let state = test_state();
        let backend = std::sync::Arc::new(MemoryBackend::new());
        backend.write("/a.txt", b"aaa").await.unwrap();
        backend.write("/b.txt", b"bbb").await.unwrap();
        backend.create_dir("/dest").await.unwrap();

        let paths = vec!["/a.txt".to_string(), "/b.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Copy, 2, "test".into(), "d".into(), "/".into()).await;

        let status = run_copy_loop(
            &state.jobs, &job_id, &cancel_flag, backend.as_ref(), &paths, "/dest",
        ).await;
        assert_eq!(status, JobStatus::Completed);
        assert_eq!(backend.read("/dest/a.txt").await.unwrap(), b"aaa");
        assert_eq!(backend.read("/dest/b.txt").await.unwrap(), b"bbb");
        // Originals still exist
        assert!(backend.exists("/a.txt").await.unwrap());
    }

    #[tokio::test]
    async fn run_copy_loop_cancelled() {
        let state = test_state();
        let backend = std::sync::Arc::new(MemoryBackend::new());
        backend.write("/a.txt", b"aaa").await.unwrap();

        let paths = vec!["/a.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Copy, 1, "test".into(), "d".into(), "/".into()).await;
        cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);

        let status = run_copy_loop(
            &state.jobs, &job_id, &cancel_flag, backend.as_ref(), &paths, "/dest",
        ).await;
        assert_eq!(status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn run_copy_loop_fails_on_missing_source() {
        let state = test_state();
        let backend = std::sync::Arc::new(MemoryBackend::new());

        let paths = vec!["/nonexistent.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Copy, 1, "test".into(), "d".into(), "/".into()).await;

        let status = run_copy_loop(
            &state.jobs, &job_id, &cancel_flag, backend.as_ref(), &paths, "/dest",
        ).await;
        assert_eq!(status, JobStatus::Failed);
    }

    // ---- run_move_loop tests ----

    #[tokio::test]
    async fn run_move_loop_moves_files() {
        let state = test_state();
        let backend = std::sync::Arc::new(MemoryBackend::new());
        backend.write("/a.txt", b"aaa").await.unwrap();
        backend.create_dir("/dest").await.unwrap();

        let paths = vec!["/a.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Move, 1, "test".into(), "d".into(), "/".into()).await;

        let status = run_move_loop(
            &state.jobs, &job_id, &cancel_flag, backend.as_ref(), &paths, "/dest",
        ).await;
        assert_eq!(status, JobStatus::Completed);
        assert_eq!(backend.read("/dest/a.txt").await.unwrap(), b"aaa");
        assert!(!backend.exists("/a.txt").await.unwrap());
    }

    #[tokio::test]
    async fn run_move_loop_cancelled() {
        let state = test_state();
        let backend = std::sync::Arc::new(MemoryBackend::new());
        backend.write("/a.txt", b"aaa").await.unwrap();

        let paths = vec!["/a.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Move, 1, "test".into(), "d".into(), "/".into()).await;
        cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);

        let status = run_move_loop(
            &state.jobs, &job_id, &cancel_flag, backend.as_ref(), &paths, "/dest",
        ).await;
        assert_eq!(status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn run_move_loop_fails_on_missing_source() {
        let state = test_state();
        let backend = std::sync::Arc::new(MemoryBackend::new());

        let paths = vec!["/nonexistent.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Move, 1, "test".into(), "d".into(), "/".into()).await;

        let status = run_move_loop(
            &state.jobs, &job_id, &cancel_flag, backend.as_ref(), &paths, "/dest",
        ).await;
        assert_eq!(status, JobStatus::Failed);
    }

    // ---- run_delete_loop tests ----

    #[tokio::test]
    async fn run_delete_loop_deletes_files() {
        let (state, backend) = test_state_with_backend("d1").await;
        backend.write("/a.txt", b"aaa").await.unwrap();
        backend.write("/b.txt", b"bbb").await.unwrap();
        state.store.index_entry("d1", "/a.txt", "a.txt", false, 3, None).unwrap();
        state.store.index_entry("d1", "/b.txt", "b.txt", false, 3, None).unwrap();

        let paths = vec!["/a.txt".to_string(), "/b.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Delete, 2, "test".into(), "d".into(), "/".into()).await;

        let status = run_delete_loop(
            &state.jobs, &job_id, &cancel_flag, backend.as_ref(), &state.store, "d1", &paths,
        ).await;
        assert_eq!(status, JobStatus::Completed);
        assert!(!backend.exists("/a.txt").await.unwrap());
        assert!(!backend.exists("/b.txt").await.unwrap());
        // Index entries should be removed
        let results = state.store.search_index("a", None, 10).unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn run_delete_loop_cancelled() {
        let (state, backend) = test_state_with_backend("d1").await;
        backend.write("/a.txt", b"aaa").await.unwrap();

        let paths = vec!["/a.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Delete, 1, "test".into(), "d".into(), "/".into()).await;
        cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);

        let status = run_delete_loop(
            &state.jobs, &job_id, &cancel_flag, backend.as_ref(), &state.store, "d1", &paths,
        ).await;
        assert_eq!(status, JobStatus::Cancelled);
        // File should still exist
        assert!(backend.exists("/a.txt").await.unwrap());
    }

    // ---- run_cross_copy_loop tests ----

    #[tokio::test]
    async fn run_cross_copy_loop_copies_between_backends() {
        let state = test_state();
        let src = std::sync::Arc::new(MemoryBackend::new());
        let dst = std::sync::Arc::new(MemoryBackend::new());
        src.write("/file.txt", b"data").await.unwrap();

        let src_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = src.clone();
        let dst_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = dst.clone();

        let paths = vec!["/file.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Copy, 1, "test".into(), "d".into(), "/".into()).await;

        let status = run_cross_copy_loop(
            &state.jobs, &job_id, &cancel_flag, &src_dyn, &dst_dyn, &paths, "/dest",
        ).await;
        assert_eq!(status, JobStatus::Completed);
        assert_eq!(dst.read("/dest/file.txt").await.unwrap(), b"data");
        assert!(src.exists("/file.txt").await.unwrap());
    }

    #[tokio::test]
    async fn run_cross_copy_loop_cancelled() {
        let state = test_state();
        let src: std::sync::Arc<dyn crate::storage::StorageBackend> = std::sync::Arc::new(MemoryBackend::new());
        let dst: std::sync::Arc<dyn crate::storage::StorageBackend> = std::sync::Arc::new(MemoryBackend::new());

        let paths = vec!["/file.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Copy, 1, "test".into(), "d".into(), "/".into()).await;
        cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);

        let status = run_cross_copy_loop(
            &state.jobs, &job_id, &cancel_flag, &src, &dst, &paths, "/dest",
        ).await;
        assert_eq!(status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn run_cross_copy_loop_fails_on_missing() {
        let state = test_state();
        let src: std::sync::Arc<dyn crate::storage::StorageBackend> = std::sync::Arc::new(MemoryBackend::new());
        let dst: std::sync::Arc<dyn crate::storage::StorageBackend> = std::sync::Arc::new(MemoryBackend::new());

        let paths = vec!["/nonexistent.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Copy, 1, "test".into(), "d".into(), "/".into()).await;

        let status = run_cross_copy_loop(
            &state.jobs, &job_id, &cancel_flag, &src, &dst, &paths, "/dest",
        ).await;
        assert_eq!(status, JobStatus::Failed);
    }

    // ---- run_cross_move_loop tests ----

    #[tokio::test]
    async fn run_cross_move_loop_moves_between_backends() {
        let state = test_state();
        let src = std::sync::Arc::new(MemoryBackend::new());
        let dst = std::sync::Arc::new(MemoryBackend::new());
        src.write("/file.txt", b"data").await.unwrap();

        let src_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = src.clone();
        let dst_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = dst.clone();

        let paths = vec!["/file.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Move, 1, "test".into(), "d".into(), "/".into()).await;

        let status = run_cross_move_loop(
            &state.jobs, &job_id, &cancel_flag, &src_dyn, &dst_dyn, &paths, "/dest",
        ).await;
        assert_eq!(status, JobStatus::Completed);
        assert_eq!(dst.read("/dest/file.txt").await.unwrap(), b"data");
        assert!(!src.exists("/file.txt").await.unwrap());
    }

    #[tokio::test]
    async fn run_cross_move_loop_cancelled() {
        let state = test_state();
        let src: std::sync::Arc<dyn crate::storage::StorageBackend> = std::sync::Arc::new(MemoryBackend::new());
        let dst: std::sync::Arc<dyn crate::storage::StorageBackend> = std::sync::Arc::new(MemoryBackend::new());

        let paths = vec!["/file.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Move, 1, "test".into(), "d".into(), "/".into()).await;
        cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);

        let status = run_cross_move_loop(
            &state.jobs, &job_id, &cancel_flag, &src, &dst, &paths, "/dest",
        ).await;
        assert_eq!(status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn run_cross_move_loop_fails_on_missing() {
        let state = test_state();
        let src: std::sync::Arc<dyn crate::storage::StorageBackend> = std::sync::Arc::new(MemoryBackend::new());
        let dst: std::sync::Arc<dyn crate::storage::StorageBackend> = std::sync::Arc::new(MemoryBackend::new());

        let paths = vec!["/nonexistent.txt".to_string()];
        let (job_id, cancel_flag) = state.jobs.create(JobKind::Move, 1, "test".into(), "d".into(), "/".into()).await;

        let status = run_cross_move_loop(
            &state.jobs, &job_id, &cancel_flag, &src, &dst, &paths, "/dest",
        ).await;
        assert_eq!(status, JobStatus::Failed);
    }

    // ---- get_folder_size tests ----

    #[tokio::test]
    async fn test_folder_size_empty_dir() {
        let (state, backend) = test_state_with_backend("d1").await;
        backend.create_dir("/empty").await.unwrap();
        let size = get_folder_size_inner(&state, "d1", "/empty").await.unwrap();
        assert_eq!(size, 0);
    }

    #[tokio::test]
    async fn test_folder_size_flat_files() {
        let (state, backend) = test_state_with_backend("d1").await;
        backend.create_dir("/flat").await.unwrap();
        backend.write("/flat/a.txt", b"hello").await.unwrap(); // 5 bytes
        backend.write("/flat/b.txt", b"world!!").await.unwrap(); // 7 bytes
        let size = get_folder_size_inner(&state, "d1", "/flat").await.unwrap();
        assert_eq!(size, 12);
    }

    #[tokio::test]
    async fn test_folder_size_nested() {
        let (state, backend) = test_state_with_backend("d1").await;
        backend.create_dir("/root").await.unwrap();
        backend.write("/root/top.txt", b"abc").await.unwrap(); // 3 bytes
        backend.create_dir("/root/sub").await.unwrap();
        backend.write("/root/sub/deep.txt", b"defgh").await.unwrap(); // 5 bytes
        backend.create_dir("/root/sub/inner").await.unwrap();
        backend.write("/root/sub/inner/file.txt", b"ij").await.unwrap(); // 2 bytes
        let size = get_folder_size_inner(&state, "d1", "/root").await.unwrap();
        assert_eq!(size, 10);
    }

    // ---- batch_rename tests ----

    #[tokio::test]
    async fn test_batch_rename_simple() {
        let (state, backend) = test_state_with_backend("d1").await;
        backend.write("/photo_001.jpg", b"img1").await.unwrap();
        backend.write("/photo_002.jpg", b"img2").await.unwrap();
        let paths = vec!["/photo_001.jpg".to_string(), "/photo_002.jpg".to_string()];
        let result = batch_rename_inner(&state, "d1", &paths, "photo", "image").await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ("/photo_001.jpg".to_string(), "/image_001.jpg".to_string()));
        assert_eq!(result[1], ("/photo_002.jpg".to_string(), "/image_002.jpg".to_string()));
        // Verify files were actually renamed on the backend
        assert!(backend.exists("/image_001.jpg").await.unwrap());
        assert!(backend.exists("/image_002.jpg").await.unwrap());
        assert!(!backend.exists("/photo_001.jpg").await.unwrap());
    }

    #[tokio::test]
    async fn test_batch_rename_empty_find_errors() {
        let (state, _) = test_state_with_backend("d1").await;
        let paths = vec!["/file.txt".to_string()];
        let result = batch_rename_inner(&state, "d1", &paths, "", "replacement").await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("empty"));
    }

    #[tokio::test]
    async fn test_batch_rename_skips_unchanged() {
        let (state, backend) = test_state_with_backend("d1").await;
        backend.write("/hello.txt", b"data").await.unwrap();
        backend.write("/world.txt", b"data").await.unwrap();
        // "xyz" is not in either filename, so both should be skipped
        let paths = vec!["/hello.txt".to_string(), "/world.txt".to_string()];
        let result = batch_rename_inner(&state, "d1", &paths, "xyz", "abc").await.unwrap();
        assert!(result.is_empty());
        // Original files should still exist
        assert!(backend.exists("/hello.txt").await.unwrap());
        assert!(backend.exists("/world.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_batch_rename_separator_in_replace_errors() {
        let (state, _) = test_state_with_backend("d1").await;
        let paths = vec!["/file.txt".to_string()];
        let result = batch_rename_inner(&state, "d1", &paths, "file", "sub/dir").await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("separator"));

        // Also test backslash
        let result2 = batch_rename_inner(&state, "d1", &paths, "file", "sub\\dir").await;
        assert!(result2.is_err());
    }

    // ---- get_size_breakdown tests ----

    #[tokio::test]
    async fn test_size_breakdown_empty_dir() {
        let (state, backend) = test_state_with_backend("d1").await;
        backend.create_dir("/empty").await.unwrap();
        let result = get_size_breakdown_inner(&state, "d1", "/empty").await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_size_breakdown_mixed_files_and_dirs() {
        let (state, backend) = test_state_with_backend("d1").await;
        backend.create_dir("/root").await.unwrap();
        backend.write("/root/big.txt", b"1234567890").await.unwrap(); // 10 bytes
        backend.write("/root/small.txt", b"ab").await.unwrap(); // 2 bytes
        backend.create_dir("/root/sub").await.unwrap();
        backend.write("/root/sub/inner.txt", b"hello").await.unwrap(); // 5 bytes

        let result = get_size_breakdown_inner(&state, "d1", "/root").await.unwrap();
        assert_eq!(result.len(), 3);

        // Sorted by size descending: big.txt (10), sub (5), small.txt (2)
        assert_eq!(result[0].name, "big.txt");
        assert_eq!(result[0].size, 10);
        assert!(!result[0].is_dir);

        assert_eq!(result[1].name, "sub");
        assert_eq!(result[1].size, 5);
        assert!(result[1].is_dir);

        assert_eq!(result[2].name, "small.txt");
        assert_eq!(result[2].size, 2);
        assert!(!result[2].is_dir);
    }

    #[tokio::test]
    async fn test_size_breakdown_returns_recursive_dir_size() {
        let (state, backend) = test_state_with_backend("d1").await;
        backend.create_dir("/root").await.unwrap();
        backend.create_dir("/root/deep").await.unwrap();
        backend.create_dir("/root/deep/nested").await.unwrap();
        backend.write("/root/deep/a.txt", b"aaa").await.unwrap(); // 3 bytes
        backend.write("/root/deep/nested/b.txt", b"bbbb").await.unwrap(); // 4 bytes

        let result = get_size_breakdown_inner(&state, "d1", "/root").await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "deep");
        assert_eq!(result[0].size, 7); // 3 + 4
        assert!(result[0].is_dir);
    }
}
