//! # Bookmark commands
//!
//! Tauri IPC command handlers for managing pinned path bookmarks.
//! Bookmarks provide quick-access links to frequently visited directories
//! across any disk.
//!
//! ## Validation
//!
//! - Labels must be non-empty and at most 255 characters.

use tauri::State;

use crate::error::DiskDeckError;
use crate::models::bookmark::Bookmark;
use crate::state::AppState;

/// Maximum allowed length for a bookmark label.
const MAX_LABEL_LENGTH: usize = 255;

/// Core logic for listing bookmarks, testable without Tauri State.
pub(crate) fn list_bookmarks_inner(
    state: &AppState,
) -> Result<Vec<Bookmark>, DiskDeckError> {
    state.store.list_bookmarks()
}

/// Core logic for adding a bookmark, testable without Tauri State.
pub(crate) fn add_bookmark_inner(
    state: &AppState,
    disk_id: &str,
    disk_name: &str,
    path: &str,
    label: &str,
) -> Result<Bookmark, DiskDeckError> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return Err(DiskDeckError::Storage(
            "Bookmark label cannot be empty".into(),
        ));
    }
    if trimmed.len() > MAX_LABEL_LENGTH {
        return Err(DiskDeckError::Storage(
            "Bookmark label too long (max 255 characters)".into(),
        ));
    }
    // Prevent duplicate bookmarks for the same disk + path
    let existing = state.store.list_bookmarks()?;
    if existing.iter().any(|b| b.disk_id == disk_id && b.path == path) {
        return Err(DiskDeckError::Storage(
            "This folder is already bookmarked".into(),
        ));
    }
    state.store.add_bookmark(disk_id, disk_name, path, trimmed)
}

/// Core logic for removing a bookmark, testable without Tauri State.
pub(crate) fn remove_bookmark_inner(
    state: &AppState,
    bookmark_id: &str,
) -> Result<(), DiskDeckError> {
    state.store.remove_bookmark(bookmark_id)
}

/// Lists all bookmarks, ordered by creation time (newest first).
///
/// # Errors
///
/// Returns errors if the database read fails.
#[tauri::command]
pub async fn list_bookmarks(
    state: State<'_, AppState>,
) -> Result<Vec<Bookmark>, DiskDeckError> {
    list_bookmarks_inner(&state)
}

/// Creates a new bookmark for a directory path on a disk.
///
/// # Arguments
///
/// * `disk_id` — UUID of the target disk.
/// * `disk_name` — Display name of the disk.
/// * `path` — Storage-relative directory path.
/// * `label` — User-facing label (non-empty, max 255 chars).
///
/// # Errors
///
/// - Label is empty or whitespace-only.
/// - Label exceeds 255 characters.
/// - Database write failure.
#[tauri::command]
pub async fn add_bookmark(
    state: State<'_, AppState>,
    disk_id: String,
    disk_name: String,
    path: String,
    label: String,
) -> Result<Bookmark, DiskDeckError> {
    add_bookmark_inner(&state, &disk_id, &disk_name, &path, &label)
}

/// Removes a bookmark by its ID.
///
/// No-op if the bookmark does not exist.
///
/// # Errors
///
/// Returns errors if the database write fails.
#[tauri::command]
pub async fn remove_bookmark(
    state: State<'_, AppState>,
    bookmark_id: String,
) -> Result<(), DiskDeckError> {
    remove_bookmark_inner(&state, &bookmark_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DiskStore;
    use crate::state::AppState;

    fn test_state() -> AppState {
        AppState::new(DiskStore::new_in_memory().unwrap())
    }

    #[test]
    fn add_and_list_bookmark() {
        let state = test_state();
        let bookmark = add_bookmark_inner(
            &state, "disk-1", "My Disk", "/docs", "Documents",
        )
        .unwrap();

        assert_eq!(bookmark.disk_id, "disk-1");
        assert_eq!(bookmark.label, "Documents");
        assert_eq!(bookmark.path, "/docs");
        assert!(!bookmark.id.is_empty());

        let bookmarks = list_bookmarks_inner(&state).unwrap();
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].id, bookmark.id);
    }

    #[test]
    fn remove_bookmark_deletes_entry() {
        let state = test_state();
        let bookmark = add_bookmark_inner(
            &state, "disk-1", "My Disk", "/docs", "Documents",
        )
        .unwrap();

        remove_bookmark_inner(&state, &bookmark.id).unwrap();

        let bookmarks = list_bookmarks_inner(&state).unwrap();
        assert!(bookmarks.is_empty());
    }

    #[test]
    fn empty_label_rejected() {
        let state = test_state();
        let result = add_bookmark_inner(&state, "d", "D", "/", "");
        assert!(result.is_err());
    }

    #[test]
    fn whitespace_only_label_rejected() {
        let state = test_state();
        let result = add_bookmark_inner(&state, "d", "D", "/", "   ");
        assert!(result.is_err());
    }

    #[test]
    fn label_too_long_rejected() {
        let state = test_state();
        let long_label = "x".repeat(256);
        let result = add_bookmark_inner(&state, "d", "D", "/", &long_label);
        assert!(result.is_err());
    }

    #[test]
    fn label_at_limit_accepted() {
        let state = test_state();
        let label = "x".repeat(255);
        let result = add_bookmark_inner(&state, "d", "D", "/", &label);
        assert!(result.is_ok());
    }

    #[test]
    fn label_trimmed_on_save() {
        let state = test_state();
        let bookmark = add_bookmark_inner(
            &state, "d", "D", "/", "  My Label  ",
        )
        .unwrap();
        assert_eq!(bookmark.label, "My Label");
    }

    #[test]
    fn duplicate_bookmark_rejected() {
        let state = test_state();
        add_bookmark_inner(&state, "disk-1", "My Disk", "/docs", "Documents").unwrap();
        let result = add_bookmark_inner(&state, "disk-1", "My Disk", "/docs", "Documents Again");
        assert!(result.is_err());
        // Only one bookmark should exist
        let bookmarks = list_bookmarks_inner(&state).unwrap();
        assert_eq!(bookmarks.len(), 1);
    }

    #[test]
    fn remove_nonexistent_bookmark_is_noop() {
        let state = test_state();
        let result = remove_bookmark_inner(&state, "nonexistent-id");
        assert!(result.is_ok());
    }
}
