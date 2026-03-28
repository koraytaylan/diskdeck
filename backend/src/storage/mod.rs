//! # Storage abstraction layer
//!
//! This module defines the [`StorageBackend`] trait — the unified interface that
//! all storage providers implement — and re-exports the concrete backend modules.
//!
//! ## Trait design
//!
//! `StorageBackend` uses **dynamic dispatch** (`Arc<dyn StorageBackend>`) rather
//! than monomorphization because:
//!
//! 1. Backends are selected at runtime based on user configuration.
//! 2. Multiple backends of different types coexist in a single `HashMap`.
//! 3. The `async_trait` macro is used because Rust does not yet stabilize
//!    async methods in traits natively with dynamic dispatch.
//!
//! The trait requires `Send + Sync` so backends can be safely shared across
//! Tokio tasks.
//!
//! ## Path conventions
//!
//! All paths passed to and returned from `StorageBackend` methods use a
//! **virtual absolute path** scheme with forward slashes:
//!
//! - `"/"` or `""` means the root of the backend.
//! - `"/documents/report.pdf"` is a file inside a `documents` folder.
//!
//! Each backend is responsible for mapping these virtual paths to its native
//! addressing scheme (filesystem paths, S3 key prefixes, FTP paths, etc.).
//!
//! ## Submodules
//!
//! - [`local`] — Local filesystem (the only backend that can be tested without
//!   external services).
//! - [`s3`] — AWS S3 (and S3-compatible stores).
//! - [`azure`] — Azure Blob Storage.
//! - [`gcs`] — Google Cloud Storage.
//! - [`sftp`] — SFTP over SSH.
//! - [`ftp`] — FTP/FTPS.

pub mod azure;
pub mod ftp;
pub mod gcs;
pub mod local;
pub mod s3;
pub mod sftp;

#[cfg(test)]
pub mod memory;

use async_trait::async_trait;

use crate::error::DiskDeckError;
use crate::models::entry::Entry;
use crate::models::search::SearchQuery;

/// Convenience alias for results returned by storage operations.
pub type StorageResult<T> = Result<T, DiskDeckError>;

/// Unified interface for all storage backends.
///
/// Each backend (local FS, S3, Azure, SFTP, FTP) implements this trait.
/// Command handlers interact exclusively through this trait, making the
/// rest of the codebase storage-agnostic.
///
/// # Error handling
///
/// All methods return [`StorageResult`], which wraps [`DiskDeckError`].
/// Backend implementations should map provider-specific errors into the
/// appropriate `DiskDeckError` variant (typically `Storage` or `Io`).
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// Lists entries (files and directories) at the given path.
    ///
    /// Returns an empty `Vec` if the directory is empty. Returns an error
    /// if the path does not exist or is not a directory.
    async fn list(&self, path: &str) -> StorageResult<Vec<Entry>>;

    /// Reads the entire contents of a file into memory.
    ///
    /// Returns an error if the path does not exist or is a directory.
    async fn read(&self, path: &str) -> StorageResult<Vec<u8>>;

    /// Writes data to a file, creating it if it does not exist or
    /// overwriting it if it does. Parent directories are created as needed
    /// (where the backend supports it).
    async fn write(&self, path: &str, data: &[u8]) -> StorageResult<()>;

    /// Deletes a file or directory. For directories, deletion is recursive
    /// (all children are removed).
    async fn delete(&self, path: &str) -> StorageResult<()>;

    /// Copies a file (or directory tree, for backends that support it)
    /// from `src` to `dst` within the same backend.
    async fn copy(&self, src: &str, dst: &str) -> StorageResult<()>;

    /// Renames or moves a file/directory from `src` to `dst`.
    ///
    /// Some backends (S3, SFTP) implement this as copy-then-delete because
    /// they lack a native rename/move operation.
    async fn rename(&self, src: &str, dst: &str) -> StorageResult<()>;

    /// Returns metadata for a single entry (file or directory).
    ///
    /// Returns [`DiskDeckError::NotFound`] if the path does not exist.
    async fn stat(&self, path: &str) -> StorageResult<Entry>;

    /// Checks whether a path exists in the backend.
    ///
    /// This is a convenience method; the default pattern is to call `stat()`
    /// and check for errors. Currently unused but retained for future use.
    #[allow(dead_code)]
    async fn exists(&self, path: &str) -> StorageResult<bool>;

    /// Searches for entries whose names match the query pattern.
    ///
    /// This is the **live search** fallback used when the FTS5 index is not
    /// available for a disk. For the local backend this walks the filesystem
    /// synchronously on a blocking thread; for remote backends it issues
    /// listing API calls.
    async fn search(&self, query: &SearchQuery) -> StorageResult<Vec<Entry>>;

    /// Creates a directory at the given path.
    ///
    /// For backends that emulate directories (S3, Azure), this creates a
    /// zero-byte marker object with a trailing slash.
    async fn create_dir(&self, path: &str) -> StorageResult<()>;
}
