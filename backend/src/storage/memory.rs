//! In-memory storage backend for testing.
//!
//! Stores files and directories in a `HashMap`. Not registered in the
//! production module tree -- only compiled under `#[cfg(test)]`.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::DiskDeckError;
use crate::models::entry::Entry;
use crate::models::search::SearchQuery;
use super::{StorageBackend, StorageResult};

/// In-memory storage backend for unit tests.
pub struct MemoryBackend {
    files: Mutex<HashMap<String, Vec<u8>>>,
    dirs: Mutex<std::collections::HashSet<String>>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        let mut dirs = std::collections::HashSet::new();
        dirs.insert("/".to_string());
        Self {
            files: Mutex::new(HashMap::new()),
            dirs: Mutex::new(dirs),
        }
    }
}

#[async_trait]
impl StorageBackend for MemoryBackend {
    async fn list(&self, path: &str) -> StorageResult<Vec<Entry>> {
        let prefix = if path == "/" || path.is_empty() {
            "/".to_string()
        } else {
            format!("{}/", path.trim_end_matches('/'))
        };
        let mut entries = Vec::new();
        let files = self.files.lock().unwrap();
        let dirs = self.dirs.lock().unwrap();
        let mut seen = std::collections::HashSet::new();

        for key in files.keys() {
            if let Some(rest) = key.strip_prefix(&prefix) {
                if !rest.contains('/') && !rest.is_empty() {
                    let name = rest.to_string();
                    if seen.insert(key.clone()) {
                        entries.push(Entry {
                            path: key.clone(),
                            name,
                            size: files[key].len() as u64,
                            modified: None,
                    created: None,
                            is_dir: false,
                            permissions: None,
                            mime_type: None,
                        });
                    }
                }
            }
        }
        for dir in dirs.iter() {
            if dir == "/" || dir == path {
                continue;
            }
            if let Some(rest) = dir.strip_prefix(&prefix) {
                if !rest.contains('/') && !rest.is_empty() && seen.insert(dir.clone()) {
                    let name = rest.to_string();
                    entries.push(Entry {
                        path: dir.clone(),
                        name,
                        size: 0,
                        modified: None,
                    created: None,
                        is_dir: true,
                        permissions: None,
                        mime_type: None,
                    });
                }
            }
        }
        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
        Ok(entries)
    }

    async fn read(&self, path: &str) -> StorageResult<Vec<u8>> {
        let files = self.files.lock().unwrap();
        files
            .get(path)
            .cloned()
            .ok_or_else(|| DiskDeckError::NotFound(format!("{path} not found")))
    }

    async fn write(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), data.to_vec());
        Ok(())
    }

    async fn delete(&self, path: &str) -> StorageResult<()> {
        let mut files = self.files.lock().unwrap();
        let mut dirs = self.dirs.lock().unwrap();
        files.remove(path);
        dirs.remove(path);
        // Remove children
        let prefix = format!("{}/", path.trim_end_matches('/'));
        files.retain(|k, _| !k.starts_with(&prefix));
        dirs.retain(|k| !k.starts_with(&prefix));
        Ok(())
    }

    async fn copy(&self, src: &str, dst: &str) -> StorageResult<()> {
        let data = self.read(src).await?;
        self.write(dst, &data).await
    }

    async fn rename(&self, src: &str, dst: &str) -> StorageResult<()> {
        let data = self.read(src).await?;
        self.write(dst, &data).await?;
        self.delete(src).await
    }

    async fn stat(&self, path: &str) -> StorageResult<Entry> {
        let files = self.files.lock().unwrap();
        let dirs = self.dirs.lock().unwrap();
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        if let Some(data) = files.get(path) {
            Ok(Entry {
                path: path.to_string(),
                name,
                size: data.len() as u64,
                modified: None,
                    created: None,
                is_dir: false,
                permissions: None,
                mime_type: None,
            })
        } else if dirs.contains(path) {
            Ok(Entry {
                path: path.to_string(),
                name,
                size: 0,
                modified: None,
                    created: None,
                is_dir: true,
                permissions: None,
                mime_type: None,
            })
        } else {
            Err(DiskDeckError::NotFound(format!("{path} not found")))
        }
    }

    async fn exists(&self, path: &str) -> StorageResult<bool> {
        Ok(self.files.lock().unwrap().contains_key(path)
            || self.dirs.lock().unwrap().contains(path))
    }

    async fn search(&self, query: &SearchQuery) -> StorageResult<Vec<Entry>> {
        let pattern = query.pattern.to_lowercase();
        let files = self.files.lock().unwrap();
        let dirs = self.dirs.lock().unwrap();
        let mut results = Vec::new();
        for (path, data) in files.iter() {
            let name = path.rsplit('/').next().unwrap_or(path);
            if name.to_lowercase().contains(&pattern) {
                results.push(Entry {
                    path: path.clone(),
                    name: name.to_string(),
                    size: data.len() as u64,
                    modified: None,
                    created: None,
                    is_dir: false,
                    permissions: None,
                    mime_type: None,
                });
            }
        }
        for dir in dirs.iter() {
            if dir == "/" {
                continue;
            }
            let name = dir.rsplit('/').next().unwrap_or(dir);
            if name.to_lowercase().contains(&pattern) {
                results.push(Entry {
                    path: dir.clone(),
                    name: name.to_string(),
                    size: 0,
                    modified: None,
                    created: None,
                    is_dir: true,
                    permissions: None,
                    mime_type: None,
                });
            }
        }
        Ok(results)
    }

    async fn create_dir(&self, path: &str) -> StorageResult<()> {
        self.dirs.lock().unwrap().insert(path.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn write_and_read_roundtrip() {
        let backend = MemoryBackend::new();
        backend.write("/test.txt", b"hello").await.unwrap();
        assert_eq!(backend.read("/test.txt").await.unwrap(), b"hello");
    }

    #[tokio::test]
    async fn read_nonexistent_errors() {
        let backend = MemoryBackend::new();
        assert!(backend.read("/nope.txt").await.is_err());
    }

    #[tokio::test]
    async fn stat_file() {
        let backend = MemoryBackend::new();
        backend.write("/test.txt", b"hello").await.unwrap();
        let entry = backend.stat("/test.txt").await.unwrap();
        assert_eq!(entry.name, "test.txt");
        assert_eq!(entry.size, 5);
        assert!(!entry.is_dir);
    }

    #[tokio::test]
    async fn stat_dir() {
        let backend = MemoryBackend::new();
        backend.create_dir("/mydir").await.unwrap();
        let entry = backend.stat("/mydir").await.unwrap();
        assert_eq!(entry.name, "mydir");
        assert!(entry.is_dir);
    }

    #[tokio::test]
    async fn stat_nonexistent_errors() {
        let backend = MemoryBackend::new();
        assert!(backend.stat("/nope").await.is_err());
    }

    #[tokio::test]
    async fn exists_file() {
        let backend = MemoryBackend::new();
        backend.write("/test.txt", b"hello").await.unwrap();
        assert!(backend.exists("/test.txt").await.unwrap());
    }

    #[tokio::test]
    async fn exists_dir() {
        let backend = MemoryBackend::new();
        backend.create_dir("/mydir").await.unwrap();
        assert!(backend.exists("/mydir").await.unwrap());
    }

    #[tokio::test]
    async fn exists_nonexistent() {
        let backend = MemoryBackend::new();
        assert!(!backend.exists("/nope").await.unwrap());
    }

    #[tokio::test]
    async fn delete_file() {
        let backend = MemoryBackend::new();
        backend.write("/test.txt", b"hello").await.unwrap();
        backend.delete("/test.txt").await.unwrap();
        assert!(!backend.exists("/test.txt").await.unwrap());
    }

    #[tokio::test]
    async fn delete_dir_with_children() {
        let backend = MemoryBackend::new();
        backend.create_dir("/dir").await.unwrap();
        backend.write("/dir/a.txt", b"a").await.unwrap();
        backend.delete("/dir").await.unwrap();
        assert!(!backend.exists("/dir").await.unwrap());
        assert!(!backend.exists("/dir/a.txt").await.unwrap());
    }

    #[tokio::test]
    async fn copy_file() {
        let backend = MemoryBackend::new();
        backend.write("/a.txt", b"data").await.unwrap();
        backend.copy("/a.txt", "/b.txt").await.unwrap();
        assert_eq!(backend.read("/b.txt").await.unwrap(), b"data");
        assert!(backend.exists("/a.txt").await.unwrap());
    }

    #[tokio::test]
    async fn rename_file() {
        let backend = MemoryBackend::new();
        backend.write("/a.txt", b"data").await.unwrap();
        backend.rename("/a.txt", "/b.txt").await.unwrap();
        assert_eq!(backend.read("/b.txt").await.unwrap(), b"data");
        assert!(!backend.exists("/a.txt").await.unwrap());
    }

    #[tokio::test]
    async fn search_finds_matching_files() {
        let backend = MemoryBackend::new();
        backend.write("/readme.md", b"r").await.unwrap();
        backend.write("/guide.txt", b"g").await.unwrap();
        let query = SearchQuery {
            pattern: "readme".to_string(),
            disk_ids: None,
            recursive: false,
        };
        let results = backend.search(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "readme.md");
    }

    #[tokio::test]
    async fn search_finds_matching_dirs() {
        let backend = MemoryBackend::new();
        backend.create_dir("/Documents").await.unwrap();
        let query = SearchQuery {
            pattern: "doc".to_string(),
            disk_ids: None,
            recursive: false,
        };
        let results = backend.search(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_dir);
    }

    #[tokio::test]
    async fn search_case_insensitive() {
        let backend = MemoryBackend::new();
        backend.write("/README.md", b"r").await.unwrap();
        let query = SearchQuery {
            pattern: "readme".to_string(),
            disk_ids: None,
            recursive: false,
        };
        let results = backend.search(&query).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn list_root_returns_top_level() {
        let backend = MemoryBackend::new();
        backend.write("/a.txt", b"a").await.unwrap();
        backend.create_dir("/sub").await.unwrap();
        backend.write("/sub/b.txt", b"b").await.unwrap();
        let entries = backend.list("/").await.unwrap();
        assert_eq!(entries.len(), 2); // a.txt and sub dir
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"a.txt"));
        assert!(names.contains(&"sub"));
    }

    #[tokio::test]
    async fn list_subdir() {
        let backend = MemoryBackend::new();
        backend.create_dir("/sub").await.unwrap();
        backend.write("/sub/b.txt", b"b").await.unwrap();
        let entries = backend.list("/sub").await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "b.txt");
    }

    #[tokio::test]
    async fn list_empty() {
        let backend = MemoryBackend::new();
        let entries = backend.list("/").await.unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn create_dir_and_list() {
        let backend = MemoryBackend::new();
        backend.create_dir("/newdir").await.unwrap();
        let entries = backend.list("/").await.unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "newdir");
    }
}
