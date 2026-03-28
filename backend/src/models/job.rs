//! # Job model types
//!
//! Defines the data structures for the job management system that tracks
//! long-running bulk file operations (copy, move, delete).
//!
//! These types are serialized across the Tauri IPC boundary as JSON, both in
//! command responses (`list_jobs`) and in `"job-update"` events emitted to the
//! frontend during operation progress.

use serde::{Deserialize, Serialize};

/// What kind of work a job performs.
///
/// Used to distinguish between different bulk operations in the frontend UI
/// (e.g., showing a copy icon vs. a trash icon in the job list).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Copying one or more entries to a new location.
    Copy,
    /// Moving (renaming) one or more entries to a new location.
    Move,
    /// Permanently deleting one or more entries.
    Delete,
}

/// Lifecycle status of a job.
///
/// Jobs transition through states in one direction:
/// `Running` → `Completed` | `Failed` | `Cancelled`.
/// Once a job leaves the `Running` state it is considered finished and will
/// not change status again.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// The job is currently processing entries.
    Running,
    /// All entries were processed successfully.
    Completed,
    /// The job encountered an error and stopped.
    Failed,
    /// The user requested cancellation and the job stopped.
    Cancelled,
}

/// Snapshot of a job's state, sent to the frontend via `"job-update"` events
/// and the `list_jobs` command.
///
/// This struct is a point-in-time snapshot — the underlying job may continue
/// to make progress after this snapshot is created. The frontend should treat
/// each received `JobInfo` as the latest known state for the given `id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    /// Unique identifier (UUID v4) for this job.
    pub id: String,
    /// What kind of operation this job performs.
    pub kind: JobKind,
    /// Current lifecycle status.
    pub status: JobStatus,
    /// Number of entries processed so far.
    pub completed: u32,
    /// Total number of entries to process.
    pub total: u32,
    /// Human-readable summary of what this job does, e.g.
    /// "readme.md → /Documents" or "3 files → /backup".
    pub description: String,
    /// Path of the entry currently being processed. Empty when finished.
    pub current_item: String,
    /// Error message if the job failed. `None` for non-failed jobs.
    pub error: Option<String>,
    /// Unix timestamp in milliseconds when the job was created.
    pub created_at: i64,
    /// Disk ID where the job's effects land (for auto-refresh).
    pub disk_id: String,
    /// Directory path where the job writes/deletes entries (for auto-refresh).
    pub target_path: String,
}
