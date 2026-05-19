//! # Application state
//!
//! Defines the shared application state that is managed by Tauri and
//! accessible to all command handlers via `State<'_, AppState>`.
//!
//! ## `AppState`
//!
//! Holds the disk registry (configs + backends), the database store, the
//! job registry, and the filesystem watcher registry.
//!
//! ## `JobRegistry`
//!
//! Thread-safe registry for tracking bulk file operations. Each job has a
//! unique ID, a cancellation flag, and a `JobInfo` snapshot that is updated
//! as the operation progresses. The registry supports concurrent reads
//! (listing jobs) and writes (creating, updating, cancelling jobs).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::db::DiskStore;
use crate::models::disk::DiskConfig;
use crate::models::job::{JobInfo, JobKind, JobStatus};
use crate::storage::StorageBackend;
use crate::watcher::WatcherRegistry;

/// Shared application state managed by Tauri.
///
/// All fields use interior mutability (`RwLock`, `Mutex`) so the state
/// can be shared across async command handlers without `&mut` access.
pub struct AppState {
    /// Registered disk configurations (id → config).
    pub disks: RwLock<Vec<DiskConfig>>,
    /// Active storage backends (disk_id → backend instance).
    /// Backends are constructed lazily on first use.
    pub backends: RwLock<HashMap<String, Arc<dyn StorageBackend>>>,
    /// Per-disk mutexes to prevent concurrent backend construction.
    /// When a disk is being lazily initialized, its ID is inserted here
    /// so concurrent callers wait instead of starting a second initialization.
    pub backend_locks: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Persistent storage for disk configs, preferences, bookmarks, search index.
    pub store: DiskStore,
    /// Registry for tracking bulk file operations.
    pub jobs: Arc<JobRegistry>,
    /// Registry for active filesystem watchers.
    pub watchers: WatcherRegistry,
}

impl AppState {
    /// Creates a new `AppState` with the given database store.
    pub fn new(store: DiskStore) -> Self {
        Self {
            disks: RwLock::new(Vec::new()),
            backends: RwLock::new(HashMap::new()),
            backend_locks: tokio::sync::Mutex::new(HashMap::new()),
            store,
            jobs: Arc::new(JobRegistry::new()),
            watchers: WatcherRegistry::new(),
        }
    }
}

/// Internal bookkeeping for a single job.
struct JobEntry {
    /// Mutable snapshot of the job's state.
    info: RwLock<JobInfo>,
    /// Cooperative cancellation flag polled by the spawned task.
    cancel_flag: Arc<AtomicBool>,
}

/// Thread-safe registry for tracking bulk file operations.
///
/// Jobs are created with [`JobRegistry::create`], which returns a job ID
/// and a cancellation flag. The spawned task updates progress via
/// [`JobRegistry::update_progress`] and completion via
/// [`JobRegistry::complete`] / [`JobRegistry::fail`].
#[derive(Clone)]
pub struct JobRegistry {
    /// All registered jobs, keyed by job ID.
    jobs: Arc<RwLock<HashMap<String, Arc<JobEntry>>>>,
}

impl JobRegistry {
    /// Creates an empty job registry.
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Creates a new running job and returns its ID and cancellation flag.
    ///
    /// The returned `cancel_flag` should be passed to the spawned task so it
    /// can poll for cancellation between items.
    pub async fn create(
        &self,
        kind: JobKind,
        total: u32,
        description: String,
        disk_id: String,
        target_path: String,
    ) -> (String, Arc<AtomicBool>) {
        let id = uuid::Uuid::new_v4().to_string();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let info = JobInfo {
            id: id.clone(),
            kind,
            status: JobStatus::Running,
            completed: 0,
            total,
            description,
            current_item: String::new(),
            error: None,
            created_at: chrono::Utc::now().timestamp_millis(),
            disk_id,
            target_path,
        };
        let entry = Arc::new(JobEntry {
            info: RwLock::new(info),
            cancel_flag: cancel_flag.clone(),
        });
        self.jobs.write().await.insert(id.clone(), entry);
        (id, cancel_flag)
    }

    /// Returns a snapshot of a job's current state.
    pub async fn get(&self, id: &str) -> Option<JobInfo> {
        let entry = {
            let jobs = self.jobs.read().await;
            jobs.get(id).cloned()
        }?;
        let info = entry.info.read().await.clone();
        Some(info)
    }

    /// Returns snapshots of all jobs, sorted by creation time (newest first).
    pub async fn list(&self) -> Vec<JobInfo> {
        let jobs = self.jobs.read().await;
        let mut infos = Vec::with_capacity(jobs.len());
        for entry in jobs.values() {
            infos.push(entry.info.read().await.clone());
        }
        infos.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        infos
    }

    /// Updates the progress of a running job.
    ///
    /// Sets `completed` to the given count and `current_item` to the
    /// path currently being processed.
    pub async fn update_progress(&self, id: &str, completed: u32, current_item: &str) {
        let jobs = self.jobs.read().await;
        if let Some(entry) = jobs.get(id) {
            let mut info = entry.info.write().await;
            info.completed = completed;
            info.current_item = current_item.to_string();
        }
    }

    /// Sets the total entry count for a job (used when the exact count
    /// is not known at creation time, e.g., archive extraction).
    pub async fn set_total(&self, id: &str, total: u32) {
        let jobs = self.jobs.read().await;
        if let Some(entry) = jobs.get(id) {
            let mut info = entry.info.write().await;
            info.total = total;
        }
    }

    /// Marks a job as completed successfully.
    pub async fn complete(&self, id: &str) {
        let jobs = self.jobs.read().await;
        if let Some(entry) = jobs.get(id) {
            let mut info = entry.info.write().await;
            info.status = JobStatus::Completed;
            info.current_item.clear();
        }
    }

    /// Marks a job as failed with the given error message.
    pub async fn fail(&self, id: &str, error: &str) {
        let jobs = self.jobs.read().await;
        if let Some(entry) = jobs.get(id) {
            let mut info = entry.info.write().await;
            info.status = JobStatus::Failed;
            info.error = Some(error.to_string());
            info.current_item.clear();
        }
    }

    /// Requests cancellation of a running job.
    ///
    /// Sets the cancellation flag, which the spawned task should poll
    /// between items. The task is responsible for setting the final
    /// status to `Cancelled`.
    pub async fn cancel(&self, id: &str) -> bool {
        let entry = {
            let jobs = self.jobs.read().await;
            jobs.get(id).cloned()
        };
        if let Some(entry) = entry {
            entry.cancel_flag.store(true, Ordering::Relaxed);
            let mut info = entry.info.write().await;
            if info.status == JobStatus::Running {
                info.status = JobStatus::Cancelled;
                info.current_item.clear();
            }
            true
        } else {
            false
        }
    }

    /// Removes all finished (non-running) jobs from the registry.
    pub async fn clear_finished(&self) {
        let mut jobs = self.jobs.write().await;
        jobs.retain(|_, entry| {
            // We need to check without await — use try_read
            if let Ok(info) = entry.info.try_read() {
                info.status == JobStatus::Running
            } else {
                true // Keep if we can't read (probably being updated)
            }
        });
    }

    /// Sets the final status of a job (completed, failed, or cancelled).
    ///
    /// This is a convenience method used by the copy/move/delete loops to
    /// set the terminal state in one call.
    pub async fn finish(&self, id: &str, status: JobStatus, error: Option<String>) {
        let jobs = self.jobs.read().await;
        if let Some(entry) = jobs.get(id) {
            let mut info = entry.info.write().await;
            info.status = status;
            info.error = error;
            info.current_item.clear();
        }
    }

    /// Returns whether the cancellation flag is set for a job.
    pub async fn is_cancelled(&self, id: &str) -> bool {
        let jobs = self.jobs.read().await;
        jobs.get(id)
            .map(|e| e.cancel_flag.load(Ordering::Relaxed))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_and_list_jobs() {
        let registry = JobRegistry::new();
        let (id1, _) = registry.create(JobKind::Copy, 5, "test".into(), "d".into(), "/".into()).await;
        let (id2, _) = registry.create(JobKind::Move, 3, "test2".into(), "d".into(), "/".into()).await;
        let jobs = registry.list().await;
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().any(|j| j.id == id1));
        assert!(jobs.iter().any(|j| j.id == id2));
    }

    #[tokio::test]
    async fn update_progress() {
        let registry = JobRegistry::new();
        let (id, _) = registry.create(JobKind::Delete, 10, "desc".into(), "d".into(), "/".into()).await;
        registry.update_progress(&id, 3, "/a/b.txt").await;
        let info = registry.get(&id).await.unwrap();
        assert_eq!(info.completed, 3);
        assert_eq!(info.current_item, "/a/b.txt");
    }

    #[tokio::test]
    async fn complete_job() {
        let registry = JobRegistry::new();
        let (id, _) = registry.create(JobKind::Copy, 5, "test".into(), "d".into(), "/".into()).await;
        registry.complete(&id).await;
        let info = registry.get(&id).await.unwrap();
        assert_eq!(info.status, JobStatus::Completed);
        assert!(info.current_item.is_empty());
    }

    #[tokio::test]
    async fn fail_job() {
        let registry = JobRegistry::new();
        let (id, _) = registry.create(JobKind::Copy, 5, "test".into(), "d".into(), "/".into()).await;
        registry.fail(&id, "something went wrong").await;
        let info = registry.get(&id).await.unwrap();
        assert_eq!(info.status, JobStatus::Failed);
        assert_eq!(info.error, Some("something went wrong".into()));
    }

    #[tokio::test]
    async fn cancel_job() {
        let registry = JobRegistry::new();
        let (id, _) = registry.create(JobKind::Move, 3, "test".into(), "d".into(), "/".into()).await;
        registry.cancel(&id).await;
        let info = registry.get(&id).await.unwrap();
        assert_eq!(info.status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn cancel_flag_is_set() {
        let registry = JobRegistry::new();
        let (id, cancel_flag) = registry.create(JobKind::Delete, 1, "test".into(), "d".into(), "/".into()).await;
        assert!(!cancel_flag.load(Ordering::Relaxed));
        registry.cancel(&id).await;
        assert!(cancel_flag.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn list_sorted_newest_first() {
        let registry = JobRegistry::new();
        registry.create(JobKind::Copy, 1, "first".into(), "d".into(), "/".into()).await;
        registry.create(JobKind::Move, 1, "second".into(), "d".into(), "/".into()).await;
        let jobs = registry.list().await;
        assert!(jobs[0].created_at >= jobs[1].created_at);
    }

    #[tokio::test]
    async fn clear_finished_removes_non_running() {
        let registry = JobRegistry::new();
        let (id1, _) = registry.create(JobKind::Copy, 1, "running".into(), "d".into(), "/".into()).await;
        let (id2, _) = registry.create(JobKind::Move, 1, "done".into(), "d".into(), "/".into()).await;
        let (id3, _) = registry.create(JobKind::Delete, 1, "failed".into(), "d".into(), "/".into()).await;
        let (id4, _) = registry.create(JobKind::Copy, 1, "cancelled".into(), "d".into(), "/".into()).await;

        registry.complete(&id2).await;
        registry.fail(&id3, "err").await;
        registry.cancel(&id4).await;

        registry.clear_finished().await;
        let jobs = registry.list().await;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, id1);
    }

    #[tokio::test]
    async fn job_info_includes_disk_and_path() {
        let registry = JobRegistry::new();
        let (id, _) = registry.create(
            JobKind::Copy, 1, "test".into(),
            "disk-123".into(), "/documents".into(),
        ).await;
        let info = registry.get(&id).await.unwrap();
        assert_eq!(info.disk_id, "disk-123");
        assert_eq!(info.target_path, "/documents");
    }
}
