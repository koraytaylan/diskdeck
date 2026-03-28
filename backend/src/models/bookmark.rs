//! # Bookmark model
//!
//! Defines the [`Bookmark`] struct representing a user-pinned path for
//! quick-access navigation in the sidebar. Bookmarks reference a specific
//! directory on a specific disk, identified by disk ID and path.

use serde::{Deserialize, Serialize};

/// A pinned directory path for quick-access navigation.
///
/// Bookmarks appear in the sidebar above the disk tree and allow users
/// to jump directly to frequently visited directories on any disk.
///
/// # Fields
///
/// - `id` — UUID v4 generated on creation.
/// - `disk_id` — The disk this bookmark points to.
/// - `disk_name` — Cached display name of the disk (avoids extra lookups).
/// - `path` — Storage-relative path to the bookmarked directory.
/// - `label` — User-facing label displayed in the sidebar.
/// - `created_at` — ISO 8601 timestamp of when the bookmark was created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// Unique identifier (UUID v4 string).
    pub id: String,
    /// ID of the disk this bookmark references.
    pub disk_id: String,
    /// Display name of the disk at the time of bookmarking.
    pub disk_name: String,
    /// Storage-relative path to the bookmarked directory.
    pub path: String,
    /// User-chosen label for this bookmark.
    pub label: String,
    /// ISO 8601 timestamp of when this bookmark was created.
    pub created_at: String,
}
