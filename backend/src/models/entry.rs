//! # File/directory entry model
//!
//! Defines the [`Entry`] struct, which is the universal representation of a
//! file or directory returned by all storage backends. This is the primary
//! data structure that the frontend renders in the file browser.

use serde::{Deserialize, Serialize};

/// File or folder metadata returned to the frontend.
///
/// All storage backends produce `Entry` instances with a consistent schema,
/// regardless of the underlying storage system.
///
/// ## Path conventions
///
/// - `path` is a virtual path relative to the backend root, always starting
///   with `/` (e.g., `/documents/report.pdf`).
/// - `name` is just the filename component (e.g., `report.pdf`).
/// - The frontend uses `path` for navigation (passing it back to `list_entries`)
///   and `name` for display.
///
/// ## Optional fields
///
/// - `modified` — Unix timestamp in seconds. `None` if the backend does not
///   provide modification times (e.g., FTP LIST parsing).
/// - `permissions` — Octal permission string (e.g., `"100644"`). Only populated
///   on Unix for local files and from FTP LIST output.
/// - `mime_type` — MIME type guessed from the file extension. `None` for
///   directories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Virtual path relative to the backend root (e.g., `/docs/file.txt`).
    pub path: String,
    /// Filename or directory name (last path component).
    pub name: String,
    /// File size in bytes. `0` for directories.
    pub size: u64,
    /// Last modification time as Unix timestamp (seconds since epoch).
    /// `None` if unavailable.
    pub modified: Option<i64>,
    /// Creation time as Unix timestamp (seconds since epoch).
    /// `None` if the platform or backend does not provide it.
    pub created: Option<i64>,
    /// `true` if this entry is a directory, `false` for files.
    pub is_dir: bool,
    /// File permissions as an octal string (Unix only). `None` on other platforms.
    pub permissions: Option<String>,
    /// MIME type guessed from file extension (e.g., `"application/pdf"`).
    /// `None` for directories.
    pub mime_type: Option<String>,
}

/// A single entry in a directory size breakdown.
///
/// Used by the disk usage treemap to display the relative size of each
/// immediate child (file or subdirectory) in a directory. For subdirectories,
/// `size` is the recursive total of all files within.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeEntry {
    /// Virtual path relative to the backend root.
    pub path: String,
    /// Filename or directory name.
    pub name: String,
    /// Size in bytes (recursive for directories).
    pub size: u64,
    /// `true` if this entry is a directory.
    pub is_dir: bool,
}
