//! # Background indexing engine
//!
//! Provides the logic for recursively walking storage backends and populating
//! the FTS5 full-text search index. This enables fast, offline-capable search
//! across all connected disks.
//!
//! ## How it works
//!
//! 1. On app startup, [`start_background_indexing`] spawns an async task that
//!    iterates over all registered disks and calls [`index_disk`] for each.
//! 2. `index_disk` sets the disk's index status to `"indexing"`, then calls
//!    [`walk_backend`] to recursively list all entries.
//! 3. The collected entries are bulk-inserted into the `search_entries` table
//!    (which has FTS5 triggers that keep `search_index` in sync).
//! 4. On success, the status is set to `"ready"` with an entry count.
//!    On failure, it is set to `"stale"`.
//!
//! ## Recursive walk
//!
//! [`walk_backend`] uses async recursion (via `Pin<Box<...>>`) to traverse the
//! directory tree. For each entry:
//! - An [`IndexEntry`] is created (with an empty `disk_id` — filled by the caller).
//! - If the entry is a directory, the function recurses into it.
//! - Errors reading a subdirectory are logged and skipped (the walk continues
//!   with remaining entries). This ensures that a single unreadable directory
//!   does not abort indexing for the entire disk.
//!
//! ## Performance considerations
//!
//! - For local disks, the walk is fast (filesystem metadata calls).
//! - For remote backends (S3, SFTP, FTP), each `list()` call is a network
//!   round-trip. Indexing a large remote disk can take minutes.
//! - The bulk insert uses a single SQLite transaction for efficiency.
//! - Indexing runs in the background and does not block the UI.

use std::pin::Pin;

use tauri::Manager;

use crate::db::IndexEntry;
use crate::error::DiskDeckError;
use crate::state::AppState;
use crate::storage::StorageBackend;

/// Spawns background indexing for all disks. Non-blocking, returns immediately.
///
/// This is called once during app startup. Each disk is indexed sequentially
/// (not in parallel) to avoid overwhelming remote backends with concurrent
/// listing requests.
///
/// Errors for individual disks are logged but do not prevent other disks
/// from being indexed.
pub fn start_background_indexing(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();
        let disks = state.disks.read().await.clone();
        for disk in disks {
            if let Err(e) = index_disk(&state, &disk.id).await {
                log::error!("Failed to index disk '{}': {}", disk.name, e);
            }
        }
    });
}

/// Performs a full index of a single disk.
///
/// Replaces all existing index entries for the disk with fresh data from
/// a complete recursive walk of the backend.
///
/// # Index status transitions
///
/// - Sets status to `"indexing"` at the start.
/// - Sets status to `"ready"` (with entry count) on success.
/// - Sets status to `"stale"` on failure.
///
/// # Errors
///
/// Returns `DiskDeckError::NotFound` if the disk's backend is not registered.
/// Returns other errors if the walk or bulk insert fails.
pub async fn index_disk(state: &AppState, disk_id: &str) -> Result<(), DiskDeckError> {
    // Set status to "indexing"
    state.store.set_index_meta(disk_id, "indexing", 0)?;

    // Only index disks that already have a constructed backend.
    // Backends are built lazily on first user interaction, so disks
    // not yet accessed are skipped here and indexed later when the
    // user first opens them (mark_stale triggers re-indexing).
    let backend = {
        let backends = state.backends.read().await;
        backends
            .get(disk_id)
            .cloned()
            .ok_or_else(|| DiskDeckError::NotFound(format!("Disk '{disk_id}' not found")))?
    };

    // Walk all entries starting from the root
    let result = walk_backend(backend.as_ref(), "/").await;

    match result {
        Ok(entries) => {
            let count = entries.len() as i64;
            state.store.index_entries_bulk(disk_id, &entries)?;
            state.store.set_index_meta(disk_id, "ready", count)?;
            Ok(())
        }
        Err(e) => {
            state.store.set_index_meta(disk_id, "stale", 0)?;
            Err(e)
        }
    }
}

/// Recursively walks a backend starting from `path`, collecting all entries.
///
/// Returns a flat `Vec<IndexEntry>` containing every file and directory found.
/// The `disk_id` field of each entry is left empty (set to `String::new()`)
/// and must be filled by the caller when inserting into the database.
///
/// Uses `Pin<Box<...>>` because async functions cannot be directly recursive
/// (the future size would be infinite). Errors reading individual subdirectories
/// are logged as warnings and skipped.
fn walk_backend<'a>(
    backend: &'a dyn StorageBackend,
    path: &'a str,
) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<IndexEntry>, DiskDeckError>> + Send + 'a>> {
    Box::pin(async move {
    let mut all = Vec::new();
    let entries = backend.list(path).await?;
    for entry in entries {
        all.push(IndexEntry {
            disk_id: String::new(), // filled by caller (index_entries_bulk)
            path: entry.path.clone(),
            name: entry.name.clone(),
            is_dir: entry.is_dir,
            size: entry.size,
            modified: entry.modified,
        });
        if entry.is_dir {
            match walk_backend(backend, &entry.path).await {
                Ok(sub) => all.extend(sub),
                Err(e) => {
                    // Log and skip unreadable directories rather than aborting
                    log::warn!("Skipping dir {}: {}", entry.path, e);
                }
            }
        }
    }
    Ok(all)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DiskStore;
    use crate::state::AppState;
    use crate::storage::memory::MemoryBackend;

    fn test_state() -> AppState {
        AppState::new(DiskStore::new_in_memory().unwrap())
    }

    #[tokio::test]
    async fn walk_backend_empty_dir() {
        let backend = MemoryBackend::new();
        let entries = walk_backend(&backend, "/").await.unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn walk_backend_flat_files() {
        let backend = MemoryBackend::new();
        backend.write("/a.txt", b"a").await.unwrap();
        backend.write("/b.txt", b"b").await.unwrap();
        let entries = walk_backend(&backend, "/").await.unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn walk_backend_recursive() {
        let backend = MemoryBackend::new();
        backend.create_dir("/sub").await.unwrap();
        backend.write("/top.txt", b"t").await.unwrap();
        backend.write("/sub/nested.txt", b"n").await.unwrap();
        let entries = walk_backend(&backend, "/").await.unwrap();
        // Should find: sub (dir), top.txt (file), nested.txt (file)
        assert_eq!(entries.len(), 3);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"sub"));
        assert!(names.contains(&"top.txt"));
        assert!(names.contains(&"nested.txt"));
    }

    #[tokio::test]
    async fn walk_backend_deeply_nested() {
        let backend = MemoryBackend::new();
        backend.create_dir("/a").await.unwrap();
        backend.create_dir("/a/b").await.unwrap();
        backend.write("/a/b/deep.txt", b"deep").await.unwrap();
        let entries = walk_backend(&backend, "/").await.unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[tokio::test]
    async fn index_disk_full_cycle() {
        let state = test_state();
        let backend = std::sync::Arc::new(MemoryBackend::new());
        backend.write("/file1.txt", b"data1").await.unwrap();
        backend.write("/file2.txt", b"data2").await.unwrap();
        state
            .backends
            .write()
            .await
            .insert("d1".to_string(), backend);

        index_disk(&state, "d1").await.unwrap();

        let meta = state.store.get_index_meta("d1").unwrap().unwrap();
        assert_eq!(meta.status, "ready");
        assert_eq!(meta.entry_count, 2);
    }

    #[tokio::test]
    async fn index_disk_unknown_returns_error() {
        let state = test_state();
        let result = index_disk(&state, "nonexistent").await;
        assert!(result.is_err());
    }
}
