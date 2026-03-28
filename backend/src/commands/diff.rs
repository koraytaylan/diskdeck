//! # Directory diff command
//!
//! Compares two directories (potentially on different disks) and returns a
//! list of differences. Entries are compared by name; files are classified
//! as added, removed, modified, or unchanged based on size and modification
//! time.

use std::collections::HashMap;
use std::sync::Arc;

use tauri::State;

use crate::error::DiskDeckError;
use crate::models::diff::{DiffEntry, DiffStatus};
use crate::models::entry::Entry;
use crate::state::AppState;
use crate::storage::StorageBackend;

use super::file::get_backend;

/// Builds a name-to-entry lookup map from a list of entries.
fn build_entry_map(entries: Vec<Entry>) -> HashMap<String, Entry> {
    entries.into_iter().map(|e| (e.name.clone(), e)).collect()
}

/// Compares two entries and returns the appropriate diff status.
///
/// Two entries are considered modified if their sizes differ, or if both
/// have modification timestamps and those timestamps differ. If both are
/// directories, they are considered unchanged at this level (recursive
/// comparison is left to the caller or a future enhancement).
fn compare_entries(src: &Entry, dst: &Entry) -> DiffStatus {
    if src.is_dir && dst.is_dir {
        return DiffStatus::Unchanged;
    }
    if src.size != dst.size {
        return DiffStatus::Modified;
    }
    if let (Some(src_mod), Some(dst_mod)) = (src.modified, dst.modified) {
        if src_mod != dst_mod {
            return DiffStatus::Modified;
        }
    }
    DiffStatus::Unchanged
}

/// Inner implementation of `diff_directories`, separated for testability.
///
/// Lists entries in both directories, builds lookup maps, and classifies
/// each entry as added (source-only), removed (destination-only), modified
/// (present in both but different), or unchanged.
pub async fn diff_directories_inner(
    src_backend: &Arc<dyn StorageBackend>,
    src_path: &str,
    dst_backend: &Arc<dyn StorageBackend>,
    dst_path: &str,
) -> Result<Vec<DiffEntry>, DiskDeckError> {
    let src_entries = src_backend.list(src_path).await?;
    let dst_entries = dst_backend.list(dst_path).await?;

    let src_map = build_entry_map(src_entries);
    let dst_map = build_entry_map(dst_entries);

    let mut results = Vec::new();

    // Check each source entry against the destination
    for (name, src_entry) in &src_map {
        match dst_map.get(name) {
            None => {
                results.push(DiffEntry {
                    path: src_entry.path.clone(),
                    name: name.clone(),
                    status: DiffStatus::Added,
                    is_dir: src_entry.is_dir,
                    src_size: Some(src_entry.size),
                    dst_size: None,
                    src_modified: src_entry.modified,
                    dst_modified: None,
                });
            }
            Some(dst_entry) => {
                let status = compare_entries(src_entry, dst_entry);
                results.push(DiffEntry {
                    path: src_entry.path.clone(),
                    name: name.clone(),
                    status,
                    is_dir: src_entry.is_dir,
                    src_size: Some(src_entry.size),
                    dst_size: Some(dst_entry.size),
                    src_modified: src_entry.modified,
                    dst_modified: dst_entry.modified,
                });
            }
        }
    }

    // Entries only in the destination
    for (name, dst_entry) in &dst_map {
        if !src_map.contains_key(name) {
            results.push(DiffEntry {
                path: dst_entry.path.clone(),
                name: name.clone(),
                status: DiffStatus::Removed,
                is_dir: dst_entry.is_dir,
                src_size: None,
                dst_size: Some(dst_entry.size),
                src_modified: None,
                dst_modified: dst_entry.modified,
            });
        }
    }

    // Sort: directories first, then alphabetically by name
    results.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));

    Ok(results)
}

/// Compares two directories (potentially on different disks) and returns
/// a list of differences.
///
/// Each entry in the result describes whether it was added (source-only),
/// removed (destination-only), modified (different size or timestamp), or
/// unchanged. The comparison is non-recursive — only immediate children
/// of the specified directories are compared.
///
/// # Arguments
///
/// * `src_disk_id` — UUID of the source disk.
/// * `src_path` — Directory path on the source disk.
/// * `dst_disk_id` — UUID of the destination disk.
/// * `dst_path` — Directory path on the destination disk.
///
/// # Errors
///
/// Returns `DiskDeckError::NotFound` if either disk is not registered.
/// Returns storage errors if either directory listing fails.
#[tauri::command]
pub async fn diff_directories(
    state: State<'_, AppState>,
    src_disk_id: String,
    src_path: String,
    dst_disk_id: String,
    dst_path: String,
) -> Result<Vec<DiffEntry>, DiskDeckError> {
    let src_backend = get_backend(&state, &src_disk_id).await?;
    let dst_backend = get_backend(&state, &dst_disk_id).await?;
    diff_directories_inner(&src_backend, &src_path, &dst_backend, &dst_path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::memory::MemoryBackend;

    /// Helper to create an `Arc<dyn StorageBackend>` from a `MemoryBackend`.
    fn arc_backend(backend: MemoryBackend) -> Arc<dyn StorageBackend> {
        Arc::new(backend)
    }

    #[tokio::test]
    async fn identical_directories_all_unchanged() {
        let src = MemoryBackend::new();
        src.write("/a.txt", b"hello").await.unwrap();
        src.write("/b.txt", b"world").await.unwrap();

        let dst = MemoryBackend::new();
        dst.write("/a.txt", b"hello").await.unwrap();
        dst.write("/b.txt", b"world").await.unwrap();

        let src = arc_backend(src);
        let dst = arc_backend(dst);

        let results = diff_directories_inner(&src, "/", &dst, "/").await.unwrap();
        assert_eq!(results.len(), 2);
        for entry in &results {
            assert_eq!(entry.status, DiffStatus::Unchanged);
        }
    }

    #[tokio::test]
    async fn source_has_extra_file_is_added() {
        let src = MemoryBackend::new();
        src.write("/a.txt", b"hello").await.unwrap();
        src.write("/extra.txt", b"only in src").await.unwrap();

        let dst = MemoryBackend::new();
        dst.write("/a.txt", b"hello").await.unwrap();

        let src = arc_backend(src);
        let dst = arc_backend(dst);

        let results = diff_directories_inner(&src, "/", &dst, "/").await.unwrap();
        assert_eq!(results.len(), 2);

        let added = results.iter().find(|e| e.name == "extra.txt").unwrap();
        assert_eq!(added.status, DiffStatus::Added);
        assert!(added.src_size.is_some());
        assert!(added.dst_size.is_none());
    }

    #[tokio::test]
    async fn dest_has_extra_file_is_removed() {
        let src = MemoryBackend::new();
        src.write("/a.txt", b"hello").await.unwrap();

        let dst = MemoryBackend::new();
        dst.write("/a.txt", b"hello").await.unwrap();
        dst.write("/extra.txt", b"only in dst").await.unwrap();

        let src = arc_backend(src);
        let dst = arc_backend(dst);

        let results = diff_directories_inner(&src, "/", &dst, "/").await.unwrap();
        assert_eq!(results.len(), 2);

        let removed = results.iter().find(|e| e.name == "extra.txt").unwrap();
        assert_eq!(removed.status, DiffStatus::Removed);
        assert!(removed.src_size.is_none());
        assert!(removed.dst_size.is_some());
    }

    #[tokio::test]
    async fn same_name_different_size_is_modified() {
        let src = MemoryBackend::new();
        src.write("/data.bin", b"short").await.unwrap();

        let dst = MemoryBackend::new();
        dst.write("/data.bin", b"much longer content").await.unwrap();

        let src = arc_backend(src);
        let dst = arc_backend(dst);

        let results = diff_directories_inner(&src, "/", &dst, "/").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, DiffStatus::Modified);
        assert_eq!(results[0].src_size, Some(5));
        assert_eq!(results[0].dst_size, Some(19));
    }

    #[tokio::test]
    async fn empty_directories_returns_empty() {
        let src = arc_backend(MemoryBackend::new());
        let dst = arc_backend(MemoryBackend::new());

        let results = diff_directories_inner(&src, "/", &dst, "/").await.unwrap();
        assert!(results.is_empty());
    }
}
