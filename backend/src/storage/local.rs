//! # Local filesystem storage backend
//!
//! Implements [`StorageBackend`] for the host machine's local filesystem.
//! This is the only backend that can operate without network access and is
//! the most common backend for desktop users.
//!
//! ## Path resolution and traversal prevention
//!
//! All incoming paths are resolved against a **root directory** configured by
//! the user. The [`LocalBackend::resolve`] method canonicalizes paths (resolving
//! symlinks and `..` segments) and then verifies the result is still under the
//! root. This prevents path-traversal attacks like `../../etc/passwd`.
//!
//! On macOS, the root is eagerly canonicalized in the constructor because the OS
//! maps `/var` to `/private/var`, and without canonicalization, `strip_prefix`
//! comparisons would fail.
//!
//! ## Entry path conventions
//!
//! Paths returned in [`Entry`] structs are always:
//! - Relative to the root directory.
//! - Prefixed with `/` (e.g., `/documents/report.pdf`).
//! - Never contain the absolute host-filesystem root.
//!
//! This means the frontend can use `entry.path` directly to drill down into
//! subdirectories without knowing the actual filesystem location.
//!
//! ## Sorting
//!
//! `list()` returns entries sorted: directories first, then alphabetically
//! (case-insensitive). This matches the convention used by all other backends.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::fs;

use super::{StorageBackend, StorageResult};
use crate::error::DiskDeckError;
use crate::models::entry::Entry;
use crate::models::search::SearchQuery;

/// Local filesystem storage backend.
///
/// Wraps a root directory and provides all file operations relative to it.
/// Constructed via [`LocalBackend::new`] and registered in [`AppState::backends`].
pub struct LocalBackend {
    /// The canonicalized root directory for this backend.
    root: PathBuf,
}

impl LocalBackend {
    /// Creates a new `LocalBackend` rooted at the given directory.
    ///
    /// The path is immediately canonicalized so that subsequent `strip_prefix`
    /// comparisons work correctly even when the OS resolves symlinks
    /// (e.g., macOS `/var` -> `/private/var`). If canonicalization fails
    /// (e.g., the path does not yet exist), the original path is used as-is.
    ///
    /// # Arguments
    ///
    /// * `root` — Absolute path to the directory this backend should expose.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        // Canonicalize so strip_prefix works even when the OS resolves symlinks
        // (e.g. macOS /var -> /private/var)
        let root = root.canonicalize().unwrap_or(root);
        Self { root }
    }

    /// Resolves a virtual path against the root, with path-traversal prevention.
    ///
    /// The algorithm handles two cases:
    /// 1. **Path exists** — canonicalize it fully and check it is under the root.
    /// 2. **Path does not exist** (e.g., a new file to be written) — canonicalize
    ///    the parent directory and append the filename, then check.
    ///
    /// # Errors
    ///
    /// Returns `DiskDeckError::Storage("Path traversal not allowed")` if the
    /// resolved path escapes the root. Returns `DiskDeckError::Io` if
    /// canonicalization fails for other OS-level reasons.
    fn resolve(&self, path: &str) -> StorageResult<PathBuf> {
        let resolved = self.root.join(path.trim_start_matches('/'));
        // Canonicalize what exists; for non-existent paths, check the parent
        let check_path = if resolved.exists() {
            resolved.canonicalize().map_err(DiskDeckError::Io)?
        } else {
            let parent = resolved
                .parent()
                .ok_or_else(|| DiskDeckError::Storage("Invalid path".into()))?;
            let canonical_parent = parent.canonicalize().map_err(DiskDeckError::Io)?;
            canonical_parent.join(
                resolved
                    .file_name()
                    .ok_or_else(|| DiskDeckError::Storage("Invalid path".into()))?,
            )
        };
        let canonical_root = self.root.canonicalize().map_err(DiskDeckError::Io)?;
        if !check_path.starts_with(&canonical_root) {
            return Err(DiskDeckError::Storage(
                "Path traversal not allowed".into(),
            ));
        }
        Ok(check_path)
    }
}

/// Converts OS file metadata into a DiskDeck [`Entry`].
///
/// The resulting `Entry.path` is relative to `root` and prefixed with `/`.
/// For example, if `root` is `/home/user/data` and `path` is
/// `/home/user/data/docs/file.txt`, the entry path will be `/docs/file.txt`.
///
/// Permissions are only populated on Unix platforms (octal mode string).
/// MIME type is guessed from the file extension for regular files.
fn metadata_to_entry(path: &Path, meta: &std::fs::Metadata, root: &Path) -> Entry {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Store path relative to root with leading slash (e.g. "/Documents/file.txt")
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path);
    let rel_str = relative.to_string_lossy();
    let entry_path = if rel_str.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", rel_str)
    };

    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    let mime_type = if meta.is_file() {
        mime_guess::from_path(path)
            .first()
            .map(|m| m.to_string())
    } else {
        None
    };

    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        Some(format!("{:o}", meta.permissions().mode()))
    };
    #[cfg(not(unix))]
    let permissions = None;

    let created = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    Entry {
        path: entry_path,
        name,
        size: meta.len(),
        modified,
        created,
        is_dir: meta.is_dir(),
        permissions,
        mime_type,
    }
}

#[async_trait]
impl StorageBackend for LocalBackend {
    async fn list(&self, path: &str) -> StorageResult<Vec<Entry>> {
        let full_path = self.resolve(path)?;
        let mut entries = Vec::new();
        let mut dir = fs::read_dir(&full_path).await.map_err(DiskDeckError::Io)?;

        while let Some(dir_entry) = dir.next_entry().await.map_err(DiskDeckError::Io)? {
            let meta = dir_entry.metadata().await.map_err(DiskDeckError::Io)?;
            entries.push(metadata_to_entry(&dir_entry.path(), &meta, &self.root));
        }

        // Directories first, then alphabetical (case-insensitive)
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(entries)
    }

    async fn read(&self, path: &str) -> StorageResult<Vec<u8>> {
        let full_path = self.resolve(path)?;
        fs::read(&full_path).await.map_err(DiskDeckError::Io)
    }

    async fn write(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        let full_path = self.resolve(path)?;
        // Ensure parent directories exist (mimics `mkdir -p`)
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(DiskDeckError::Io)?;
        }
        fs::write(&full_path, data)
            .await
            .map_err(DiskDeckError::Io)
    }

    async fn delete(&self, path: &str) -> StorageResult<()> {
        let full_path = self.resolve(path)?;
        let meta = fs::metadata(&full_path)
            .await
            .map_err(DiskDeckError::Io)?;
        if meta.is_dir() {
            fs::remove_dir_all(&full_path)
                .await
                .map_err(DiskDeckError::Io)
        } else {
            fs::remove_file(&full_path)
                .await
                .map_err(DiskDeckError::Io)
        }
    }

    async fn copy(&self, src: &str, dst: &str) -> StorageResult<()> {
        let src_path = self.resolve(src)?;
        let dst_path = self.resolve(dst)?;
        let meta = fs::metadata(&src_path)
            .await
            .map_err(DiskDeckError::Io)?;

        if meta.is_dir() {
            copy_dir_recursive(&src_path, &dst_path).await?;
        } else {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(DiskDeckError::Io)?;
            }
            fs::copy(&src_path, &dst_path)
                .await
                .map_err(DiskDeckError::Io)?;
        }
        Ok(())
    }

    async fn rename(&self, src: &str, dst: &str) -> StorageResult<()> {
        let src_path = self.resolve(src)?;
        let dst_path = self.resolve(dst)?;
        if let Some(parent) = dst_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(DiskDeckError::Io)?;
        }
        fs::rename(&src_path, &dst_path)
            .await
            .map_err(DiskDeckError::Io)
    }

    async fn stat(&self, path: &str) -> StorageResult<Entry> {
        let full_path = self.resolve(path)?;
        let meta = fs::metadata(&full_path)
            .await
            .map_err(DiskDeckError::Io)?;
        Ok(metadata_to_entry(&full_path, &meta, &self.root))
    }

    async fn exists(&self, path: &str) -> StorageResult<bool> {
        let full_path = self.resolve(path)?;
        Ok(full_path.exists())
    }

    async fn search(&self, query: &SearchQuery) -> StorageResult<Vec<Entry>> {
        let root = self.root.clone();
        let pattern = query.pattern.to_lowercase();
        let recursive = query.recursive;

        // Run blocking filesystem walk on the Tokio blocking thread pool
        // to avoid starving the async runtime's cooperative scheduler.
        let root_for_prefix = self.root.clone();
        let entries = tokio::task::spawn_blocking(move || {
            let mut results = Vec::new();
            search_dir(&root, &pattern, recursive, &root_for_prefix, &mut results);
            results
        })
        .await
        .map_err(|e| DiskDeckError::Storage(e.to_string()))?;

        Ok(entries)
    }

    async fn create_dir(&self, path: &str) -> StorageResult<()> {
        let full_path = self.resolve(path)?;
        fs::create_dir_all(&full_path)
            .await
            .map_err(DiskDeckError::Io)
    }
}

/// Recursively copies a directory tree from `src` to `dst`.
///
/// Uses `Box::pin` for the recursive call because async functions cannot
/// be directly recursive without boxing the future.
async fn copy_dir_recursive(src: &Path, dst: &Path) -> StorageResult<()> {
    fs::create_dir_all(dst).await.map_err(DiskDeckError::Io)?;
    let mut dir = fs::read_dir(src).await.map_err(DiskDeckError::Io)?;

    while let Some(entry) = dir.next_entry().await.map_err(DiskDeckError::Io)? {
        let entry_path = entry.path();
        let dest_path = dst.join(entry.file_name());
        let meta = entry.metadata().await.map_err(DiskDeckError::Io)?;

        if meta.is_dir() {
            Box::pin(copy_dir_recursive(&entry_path, &dest_path)).await?;
        } else {
            fs::copy(&entry_path, &dest_path)
                .await
                .map_err(DiskDeckError::Io)?;
        }
    }
    Ok(())
}

/// Synchronous recursive directory search used by [`LocalBackend::search`].
///
/// Walks `dir`, checking each entry's name against `pattern` (case-insensitive
/// substring match). If `recursive` is true, descends into subdirectories.
///
/// This runs on a blocking thread pool and collects results into `results`.
/// Errors reading individual entries are silently skipped (the walk continues).
fn search_dir(dir: &Path, pattern: &str, recursive: bool, root: &Path, results: &mut Vec<Entry>) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };

        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.contains(pattern) {
            results.push(metadata_to_entry(&path, &meta, root));
        }

        if recursive && meta.is_dir() {
            search_dir(&path, pattern, recursive, root, results);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup() -> (TempDir, LocalBackend) {
        let dir = TempDir::new().unwrap();
        let backend = LocalBackend::new(dir.path());
        (dir, backend)
    }

    #[tokio::test]
    async fn test_write_and_read() {
        let (_dir, backend) = setup().await;
        backend.write("test.txt", b"hello world").await.unwrap();
        let content = backend.read("test.txt").await.unwrap();
        assert_eq!(content, b"hello world");
    }

    #[tokio::test]
    async fn test_list_entries() {
        let (_dir, backend) = setup().await;
        backend.write("a.txt", b"a").await.unwrap();
        backend.write("b.txt", b"b").await.unwrap();
        backend.create_dir("subdir").await.unwrap();

        let entries = backend.list("").await.unwrap();
        assert_eq!(entries.len(), 3);
        // Directories come first
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "subdir");
    }

    #[tokio::test]
    async fn test_entry_paths_are_relative_to_root() {
        let (_dir, backend) = setup().await;
        backend.write("file.txt", b"data").await.unwrap();
        backend.create_dir("docs").await.unwrap();
        backend.write("docs/readme.md", b"hi").await.unwrap();

        // Root-level entries should have paths like /file.txt, /docs
        let root_entries = backend.list("/").await.unwrap();
        let file = root_entries.iter().find(|e| e.name == "file.txt").unwrap();
        assert_eq!(file.path, "/file.txt");
        let docs = root_entries.iter().find(|e| e.name == "docs").unwrap();
        assert_eq!(docs.path, "/docs");

        // Nested entries should have paths like /docs/readme.md
        let nested = backend.list("/docs").await.unwrap();
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].path, "/docs/readme.md");
        assert_eq!(nested[0].name, "readme.md");
    }

    #[tokio::test]
    async fn test_entry_paths_never_contain_absolute_root() {
        let (_dir, backend) = setup().await;
        backend.create_dir("a").await.unwrap();
        backend.create_dir("a/b").await.unwrap();
        backend.write("a/b/c.txt", b"c").await.unwrap();

        // Navigate down two levels; path must stay relative
        let entries = backend.list("/a/b").await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "/a/b/c.txt");
        assert!(entries[0].path.starts_with('/'));
        // Must not contain the temp dir absolute path
        assert!(!entries[0].path.contains("tmp"), "path should not contain absolute root");
    }

    #[tokio::test]
    async fn test_stat_returns_relative_path() {
        let (_dir, backend) = setup().await;
        backend.create_dir("deep").await.unwrap();
        backend.create_dir("deep/nested").await.unwrap();
        backend.write("deep/nested/file.txt", b"x").await.unwrap();
        let entry = backend.stat("deep/nested/file.txt").await.unwrap();
        assert_eq!(entry.path, "/deep/nested/file.txt");
        assert_eq!(entry.name, "file.txt");
    }

    #[tokio::test]
    async fn test_search_returns_relative_paths() {
        let (_dir, backend) = setup().await;
        backend.create_dir("alpha").await.unwrap();
        backend.write("alpha/target.txt", b"t").await.unwrap();
        backend.write("other.txt", b"o").await.unwrap();

        let query = SearchQuery {
            pattern: "target".into(),
            disk_ids: None,
            recursive: true,
        };
        let results = backend.search(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "/alpha/target.txt");
    }

    #[tokio::test]
    async fn test_list_then_navigate_roundtrip() {
        // Simulates the frontend double-click flow: list root, get entry.path, list that path
        let (_dir, backend) = setup().await;
        backend.create_dir("projects").await.unwrap();
        backend.write("projects/code.rs", b"fn main() {}").await.unwrap();

        // Step 1: List root
        let root_entries = backend.list("/").await.unwrap();
        let projects = root_entries.iter().find(|e| e.name == "projects").unwrap();
        assert_eq!(projects.path, "/projects");

        // Step 2: Use the entry's path to drill down (this is what the frontend does)
        let sub_entries = backend.list(&projects.path).await.unwrap();
        assert_eq!(sub_entries.len(), 1);
        assert_eq!(sub_entries[0].name, "code.rs");
        assert_eq!(sub_entries[0].path, "/projects/code.rs");

        // Step 3: Stat using the sub-entry path
        let stat = backend.stat(&sub_entries[0].path).await.unwrap();
        assert_eq!(stat.name, "code.rs");
        assert_eq!(stat.size, 12);
    }

    #[tokio::test]
    async fn test_delete_file() {
        let (_dir, backend) = setup().await;
        backend.write("del.txt", b"data").await.unwrap();
        assert!(backend.exists("del.txt").await.unwrap());
        backend.delete("del.txt").await.unwrap();
        assert!(!backend.exists("del.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_copy_file() {
        let (_dir, backend) = setup().await;
        backend.write("src.txt", b"copy me").await.unwrap();
        backend.copy("src.txt", "dst.txt").await.unwrap();
        let content = backend.read("dst.txt").await.unwrap();
        assert_eq!(content, b"copy me");
        // Source still exists
        assert!(backend.exists("src.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_rename() {
        let (_dir, backend) = setup().await;
        backend.write("old.txt", b"data").await.unwrap();
        backend.rename("old.txt", "new.txt").await.unwrap();
        assert!(!backend.exists("old.txt").await.unwrap());
        let content = backend.read("new.txt").await.unwrap();
        assert_eq!(content, b"data");
    }

    #[tokio::test]
    async fn test_stat() {
        let (_dir, backend) = setup().await;
        backend.write("info.txt", b"12345").await.unwrap();
        let entry = backend.stat("info.txt").await.unwrap();
        assert_eq!(entry.name, "info.txt");
        assert_eq!(entry.size, 5);
        assert!(!entry.is_dir);
    }

    #[tokio::test]
    async fn test_search() {
        let (_dir, backend) = setup().await;
        backend.write("readme.md", b"r").await.unwrap();
        backend.write("readme.txt", b"r").await.unwrap();
        backend.write("other.txt", b"o").await.unwrap();

        let query = SearchQuery {
            pattern: "readme".into(),
            disk_ids: None,
            recursive: true,
        };
        let results = backend.search(&query).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_path_traversal_blocked() {
        let (_dir, backend) = setup().await;
        let result = backend.read("../../etc/passwd").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_dir() {
        let (_dir, backend) = setup().await;
        backend.create_dir("new_folder").await.unwrap();
        let entry = backend.stat("new_folder").await.unwrap();
        assert!(entry.is_dir);
    }

    #[tokio::test]
    async fn test_copy_dir_recursive() {
        let (_dir, backend) = setup().await;
        backend.create_dir("src_dir").await.unwrap();
        backend.write("src_dir/a.txt", b"a").await.unwrap();
        backend.create_dir("src_dir/nested").await.unwrap();
        backend
            .write("src_dir/nested/b.txt", b"b")
            .await
            .unwrap();

        backend.copy("src_dir", "dst_dir").await.unwrap();

        let content = backend.read("dst_dir/a.txt").await.unwrap();
        assert_eq!(content, b"a");
        let content = backend.read("dst_dir/nested/b.txt").await.unwrap();
        assert_eq!(content, b"b");
    }

    #[tokio::test]
    async fn test_rename_preserves_content() {
        let (_dir, backend) = setup().await;
        backend.write("original.txt", b"content").await.unwrap();
        backend.rename("original.txt", "renamed.txt").await.unwrap();
        let content = backend.read("renamed.txt").await.unwrap();
        assert_eq!(content, b"content");
        assert!(!backend.exists("original.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_write_overwrite_existing() {
        let (_dir, backend) = setup().await;
        backend.write("file.txt", b"v1").await.unwrap();
        backend.write("file.txt", b"v2").await.unwrap();
        let content = backend.read("file.txt").await.unwrap();
        assert_eq!(content, b"v2");
    }

    #[tokio::test]
    async fn test_delete_directory() {
        let (_dir, backend) = setup().await;
        backend.create_dir("mydir").await.unwrap();
        backend.write("mydir/file.txt", b"data").await.unwrap();
        // Deleting the dir should fail if not empty (depends on implementation)
        // But we can delete the file first then the dir
        backend.delete("mydir/file.txt").await.unwrap();
        backend.delete("mydir").await.unwrap();
        assert!(!backend.exists("mydir").await.unwrap());
    }

    #[tokio::test]
    async fn test_exists_nonexistent() {
        let (_dir, backend) = setup().await;
        assert!(!backend.exists("nope.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_read_nonexistent_errors() {
        let (_dir, backend) = setup().await;
        let result = backend.read("nope.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cross_backend_copy_file() {
        let src_dir = TempDir::new().unwrap();
        let dst_dir = TempDir::new().unwrap();
        let src = std::sync::Arc::new(LocalBackend::new(src_dir.path()));
        let dst = std::sync::Arc::new(LocalBackend::new(dst_dir.path()));

        src.write("hello.txt", b"cross disk").await.unwrap();

        // Simulate cross_copy_single logic
        let data = src.read("hello.txt").await.unwrap();
        dst.write("hello.txt", &data).await.unwrap();

        let content = dst.read("hello.txt").await.unwrap();
        assert_eq!(content, b"cross disk");
        // Source still exists
        assert!(src.exists("hello.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_cross_backend_copy_directory() {
        let src_dir = TempDir::new().unwrap();
        let dst_dir = TempDir::new().unwrap();
        let src = std::sync::Arc::new(LocalBackend::new(src_dir.path()));
        let dst = std::sync::Arc::new(LocalBackend::new(dst_dir.path()));

        src.create_dir("folder").await.unwrap();
        src.write("folder/a.txt", b"aaa").await.unwrap();
        src.write("folder/b.txt", b"bbb").await.unwrap();

        // Simulate recursive cross-copy
        dst.create_dir("folder").await.unwrap();
        let children = src.list("folder").await.unwrap();
        for child in &children {
            let data = src.read(&child.path).await.unwrap();
            dst.write(&format!("/folder/{}", child.name), &data).await.unwrap();
        }

        let content_a = dst.read("folder/a.txt").await.unwrap();
        let content_b = dst.read("folder/b.txt").await.unwrap();
        assert_eq!(content_a, b"aaa");
        assert_eq!(content_b, b"bbb");
    }

}
