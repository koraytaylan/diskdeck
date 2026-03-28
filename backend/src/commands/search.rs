//! # Search commands
//!
//! Implements Tauri IPC commands for searching across storage backends.
//!
//! ## FTS5 vs live fallback strategy
//!
//! DiskDeck uses a **dual-path search** strategy:
//!
//! 1. **FTS5 fast path** — For disks whose index status is `"ready"`, search
//!    queries are routed to the SQLite FTS5 full-text index. This is O(1)-ish
//!    for typical queries and supports prefix matching.
//!
//! 2. **Live fallback** — For disks whose index is `"stale"`, `"indexing"`, or
//!    missing, the search falls back to calling [`StorageBackend::search`] on
//!    each unindexed disk. This walks the backend in real time, which can be
//!    slow for large or remote backends.
//!
//! Both paths run for a single query if some disks are indexed and others are
//! not. Results are merged and grouped by disk.
//!
//! ## Search scoping
//!
//! The [`SearchQuery::disk_ids`] field allows the frontend to scope a search
//! to specific disks. If `None`, all disks are searched.
//!
//! ## Query length limit
//!
//! Patterns longer than 1000 characters are rejected to prevent abuse of the
//! FTS5 tokenizer.
//!
//! ## Reindexing
//!
//! `reindex_disk` triggers a full re-walk of a single disk's backend, replacing
//! all entries in the FTS5 index. This can be called manually by the user or
//! automatically after operations that invalidate the index.

use std::collections::HashMap;

use tauri::State;

use crate::db::IndexMeta;
use crate::error::DiskDeckError;
use crate::models::entry::Entry;
use crate::models::search::{SearchQuery, SearchResult};
use crate::state::AppState;

/// Core search logic, testable without Tauri State.
pub(crate) async fn search_entries_inner(
    state: &AppState,
    query: &SearchQuery,
) -> Result<Vec<SearchResult>, DiskDeckError> {
    if query.pattern.len() > 1000 {
        return Err(DiskDeckError::Storage("Search pattern too long".into()));
    }
    let disks = state.disks.read().await;

    // Partition disks into indexed (FTS) vs unindexed (live fallback)
    let mut indexed_ids: Vec<String> = Vec::new();
    let mut unindexed_ids: Vec<String> = Vec::new();

    for disk in disks.iter() {
        // Skip disks not in the query scope
        if let Some(ref ids) = query.disk_ids {
            if !ids.contains(&disk.id) {
                continue;
            }
        }
        match state.store.get_index_meta(&disk.id) {
            Ok(Some(ref meta)) if meta.status == "ready" => {
                indexed_ids.push(disk.id.clone());
            }
            _ => {
                unindexed_ids.push(disk.id.clone());
            }
        }
    }

    let mut results = Vec::new();

    // Fast path: FTS5 search for indexed disks
    if !indexed_ids.is_empty() {
        if let Ok(hits) = state.store.search_index(&query.pattern, Some(&indexed_ids), 500) {
            // Group FTS results by disk ID
            let mut grouped: HashMap<String, Vec<Entry>> = HashMap::new();
            for hit in hits {
                grouped
                    .entry(hit.disk_id.clone())
                    .or_default()
                    .push(Entry {
                        path: hit.path,
                        name: hit.name,
                        size: hit.size,
                        modified: hit.modified,
                    created: None,
                        is_dir: hit.is_dir,
                        permissions: None,
                        mime_type: None,
                    });
            }
            for (disk_id, entries) in grouped {
                let disk_name = disks
                    .iter()
                    .find(|d| d.id == disk_id)
                    .map(|d| d.name.clone())
                    .unwrap_or_default();
                results.push(SearchResult {
                    disk_id,
                    disk_name,
                    entries,
                });
            }
        }
    }

    // Fallback: live search for unindexed disks
    if !unindexed_ids.is_empty() {
        let backends = state.backends.read().await;
        for disk_id in &unindexed_ids {
            if let Some(backend) = backends.get(disk_id) {
                match backend.search(query).await {
                    Ok(entries) if !entries.is_empty() => {
                        let disk_name = disks
                            .iter()
                            .find(|d| d.id == *disk_id)
                            .map(|d| d.name.clone())
                            .unwrap_or_default();
                        results.push(SearchResult {
                            disk_id: disk_id.clone(),
                            disk_name,
                            entries,
                        });
                    }
                    Err(e) => {
                        log::warn!("Search error on disk '{}': {}", disk_id, e);
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(results)
}

/// Searches for entries matching a pattern across one or more disks.
///
/// Returns results grouped by disk. Uses FTS5 for indexed disks and
/// falls back to live backend search for unindexed disks.
#[tauri::command]
pub async fn search_entries(
    state: State<'_, AppState>,
    query: SearchQuery,
) -> Result<Vec<SearchResult>, DiskDeckError> {
    search_entries_inner(&state, &query).await
}

/// Triggers a full reindex of a single disk.
///
/// This walks the entire backend recursively and replaces all entries in the
/// FTS5 index. The index status transitions: `stale` -> `indexing` -> `ready`.
///
/// This is an IPC wrapper around [`crate::indexer::index_disk`].
/// Core logic for reindexing a disk, testable without Tauri State.
pub(crate) async fn reindex_disk_inner(
    state: &AppState,
    disk_id: &str,
) -> Result<(), DiskDeckError> {
    crate::indexer::index_disk(state, disk_id).await
}

#[tauri::command]
pub async fn reindex_disk(
    state: State<'_, AppState>,
    disk_id: String,
) -> Result<(), DiskDeckError> {
    reindex_disk_inner(&state, &disk_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DiskStore;
    use crate::models::disk::{DiskConfig, DiskType};
    use crate::storage::memory::MemoryBackend;
    use crate::storage::StorageBackend;

    fn test_state() -> AppState {
        AppState::new(DiskStore::new_in_memory().unwrap())
    }

    #[test]
    fn search_index_returns_results() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .index_entry("d1", "/readme.md", "readme.md", false, 100, None)
            .unwrap();
        store
            .index_entry("d1", "/docs/guide.txt", "guide.txt", false, 200, None)
            .unwrap();
        let results = store.search_index("readme", None, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "readme.md");
    }

    #[test]
    fn search_index_scoped_by_disk() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .index_entry("d1", "/a.txt", "a.txt", false, 10, None)
            .unwrap();
        store
            .index_entry("d2", "/a.txt", "a.txt", false, 10, None)
            .unwrap();
        let results = store
            .search_index("a", Some(&["d1".to_string()]), 100)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].disk_id, "d1");
    }

    #[test]
    fn search_index_empty_pattern() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .index_entry("d1", "/a.txt", "a.txt", false, 10, None)
            .unwrap();
        // Empty string search should return empty results
        let results = store.search_index("", None, 100).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_index_no_matches() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .index_entry("d1", "/a.txt", "a.txt", false, 10, None)
            .unwrap();
        let results = store.search_index("zzz", None, 100).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_index_empty_disk_ids() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .index_entry("d1", "/a.txt", "a.txt", false, 10, None)
            .unwrap();
        let empty: Vec<String> = vec![];
        let results = store.search_index("a", Some(&empty), 100).unwrap();
        assert!(results.is_empty());
    }

    // ---- search_entries_inner tests ----

    #[tokio::test]
    async fn search_entries_inner_pattern_too_long() {
        let state = test_state();
        let query = SearchQuery {
            pattern: "x".repeat(1001),
            disk_ids: None,
            recursive: false,
        };
        let result = search_entries_inner(&state, &query).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn search_entries_inner_fts_path() {
        let state = test_state();
        // Add a disk to state
        let disk = DiskConfig {
            id: "d1".into(),
            name: "TestDisk".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({"root": "/tmp"}),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        state.disks.write().await.push(disk);
        // Index entries and mark index as ready
        state.store.index_entry("d1", "/readme.md", "readme.md", false, 100, None).unwrap();
        state.store.set_index_meta("d1", "ready", 1).unwrap();

        let query = SearchQuery {
            pattern: "readme".into(),
            disk_ids: None,
            recursive: false,
        };
        let results = search_entries_inner(&state, &query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].disk_id, "d1");
        assert_eq!(results[0].disk_name, "TestDisk");
        assert_eq!(results[0].entries.len(), 1);
        assert_eq!(results[0].entries[0].name, "readme.md");
    }

    #[tokio::test]
    async fn search_entries_inner_live_fallback() {
        let state = test_state();
        // Add a disk without a ready index — forces live fallback
        let disk = DiskConfig {
            id: "d1".into(),
            name: "MemDisk".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({"root": "/tmp"}),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        state.disks.write().await.push(disk);
        // Register a memory backend with a file
        let backend = std::sync::Arc::new(MemoryBackend::new());
        backend.write("/notes.txt", b"hello").await.unwrap();
        let backend_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = backend;
        state.backends.write().await.insert("d1".into(), backend_dyn);

        let query = SearchQuery {
            pattern: "notes".into(),
            disk_ids: None,
            recursive: false,
        };
        let results = search_entries_inner(&state, &query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].disk_name, "MemDisk");
        assert_eq!(results[0].entries[0].name, "notes.txt");
    }

    #[tokio::test]
    async fn search_entries_inner_scoped_to_disk() {
        let state = test_state();
        let d1 = DiskConfig {
            id: "d1".into(),
            name: "Disk1".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({"root": "/tmp"}),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let d2 = DiskConfig {
            id: "d2".into(),
            name: "Disk2".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({"root": "/tmp"}),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        state.disks.write().await.extend(vec![d1, d2]);
        state.store.index_entry("d1", "/a.txt", "a.txt", false, 10, None).unwrap();
        state.store.index_entry("d2", "/a.txt", "a.txt", false, 10, None).unwrap();
        state.store.set_index_meta("d1", "ready", 1).unwrap();
        state.store.set_index_meta("d2", "ready", 1).unwrap();

        let query = SearchQuery {
            pattern: "a".into(),
            disk_ids: Some(vec!["d1".into()]),
            recursive: false,
        };
        let results = search_entries_inner(&state, &query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].disk_id, "d1");
    }

    #[tokio::test]
    async fn search_entries_inner_no_disks_returns_empty() {
        let state = test_state();
        let query = SearchQuery {
            pattern: "anything".into(),
            disk_ids: None,
            recursive: false,
        };
        let results = search_entries_inner(&state, &query).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn search_entries_inner_live_no_match_returns_empty() {
        let state = test_state();
        let disk = DiskConfig {
            id: "d1".into(),
            name: "MemDisk".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({"root": "/tmp"}),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        state.disks.write().await.push(disk);
        let backend = std::sync::Arc::new(MemoryBackend::new());
        backend.write("/other.txt", b"data").await.unwrap();
        let backend_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = backend;
        state.backends.write().await.insert("d1".into(), backend_dyn);

        let query = SearchQuery {
            pattern: "zzzzz".into(),
            disk_ids: None,
            recursive: false,
        };
        let results = search_entries_inner(&state, &query).await.unwrap();
        assert!(results.is_empty());
    }

    // ---- get_index_status_inner tests ----

    #[test]
    fn get_index_status_inner_empty() {
        let state = test_state();
        let statuses = get_index_status_inner(&state).unwrap();
        assert!(statuses.is_empty());
    }

    #[test]
    fn get_index_status_inner_with_data() {
        let state = test_state();
        state.store.set_index_meta("d1", "ready", 10).unwrap();
        state.store.set_index_meta("d2", "stale", 0).unwrap();
        let statuses = get_index_status_inner(&state).unwrap();
        assert_eq!(statuses.len(), 2);
    }

    // ---- reindex_disk_inner tests ----

    #[tokio::test]
    async fn reindex_disk_inner_with_backend() {
        let state = test_state();
        let backend = std::sync::Arc::new(MemoryBackend::new());
        backend.write("/readme.md", b"hello").await.unwrap();
        let backend_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = backend;
        state.backends.write().await.insert("d1".into(), backend_dyn);

        reindex_disk_inner(&state, "d1").await.unwrap();
        // After reindex, the entry should be in the index
        let results = state.store.search_index("readme", None, 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn reindex_disk_inner_unknown_disk_errors() {
        let state = test_state();
        let result = reindex_disk_inner(&state, "nonexistent").await;
        assert!(result.is_err());
    }
}

/// Core logic for getting index status, testable without Tauri State.
pub(crate) fn get_index_status_inner(
    state: &AppState,
) -> Result<Vec<IndexMeta>, DiskDeckError> {
    state.store.get_all_index_meta()
}

/// Returns the current index status for all disks.
///
/// Each entry contains the disk ID, status (`"ready"`, `"indexing"`, `"stale"`),
/// entry count, and last-indexed timestamp.
#[tauri::command]
pub async fn get_index_status(
    state: State<'_, AppState>,
) -> Result<Vec<IndexMeta>, DiskDeckError> {
    get_index_status_inner(&state)
}
