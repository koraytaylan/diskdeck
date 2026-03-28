//! # Directory diff model
//!
//! Types for comparing two directories (potentially on different disks) and
//! describing the differences. Used by the `diff_directories` command to return
//! a structured comparison result to the frontend.

use serde::{Deserialize, Serialize};

/// The status of a file in a directory comparison.
///
/// Determined by comparing entries from a source directory against entries
/// in a destination directory. The comparison uses entry names as the key,
/// and file size / modification time as the equality criteria.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    /// File exists only in the source.
    Added,
    /// File exists only in the destination.
    Removed,
    /// File exists in both but differs (size or modified time).
    Modified,
    /// File exists in both and is identical.
    Unchanged,
}

/// A single entry in a directory diff result.
///
/// Contains the comparison status plus metadata from both the source and
/// destination sides, allowing the frontend to display a detailed comparison
/// table with size and timestamp differences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffEntry {
    /// Virtual path relative to the comparison root.
    pub path: String,
    /// Filename or directory name (last path component).
    pub name: String,
    /// Whether this entry is added, removed, modified, or unchanged.
    pub status: DiffStatus,
    /// `true` if this entry is a directory.
    pub is_dir: bool,
    /// File size in bytes on the source side, or `None` if absent from source.
    pub src_size: Option<u64>,
    /// File size in bytes on the destination side, or `None` if absent from destination.
    pub dst_size: Option<u64>,
    /// Last modification timestamp (seconds since epoch) on the source side.
    pub src_modified: Option<i64>,
    /// Last modification timestamp (seconds since epoch) on the destination side.
    pub dst_modified: Option<i64>,
}
