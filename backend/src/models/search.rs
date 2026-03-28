//! # Search query and result models
//!
//! Defines the data structures for cross-backend search operations.
//! These are used by the search commands ([`crate::commands::search`]) and
//! the storage backend `search()` method.

use serde::{Deserialize, Serialize};

use super::entry::Entry;

/// Query parameters for cross-backend search.
///
/// Sent from the frontend as part of the `search_entries` IPC call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    /// The search pattern (case-insensitive substring match for live search,
    /// FTS5 tokenized prefix match for indexed search).
    pub pattern: String,
    /// Optional list of disk IDs to restrict the search to.
    /// If `None`, all disks are searched.
    pub disk_ids: Option<Vec<String>>,
    /// Whether to search recursively into subdirectories.
    /// Only affects live backend search (not FTS5, which is always global).
    pub recursive: bool,
}

/// Search results grouped by disk.
///
/// Each `SearchResult` represents the matches found on a single disk.
/// The frontend receives a `Vec<SearchResult>` and renders results
/// grouped by disk name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The disk ID these results belong to.
    pub disk_id: String,
    /// The human-readable disk name (for display in the UI).
    pub disk_name: String,
    /// Matching entries on this disk.
    pub entries: Vec<Entry>,
}
