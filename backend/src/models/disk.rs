//! # Disk configuration model
//!
//! Defines the data structures for disk (storage profile) configurations.
//! Each disk represents a connection to a storage backend (local folder,
//! S3 bucket, Azure container, SFTP server, or FTP server).
//!
//! These structs are persisted in the SQLCipher database and sent over
//! IPC to the frontend (with credentials redacted — see
//! [`crate::commands::disk::redact_config`]).

use serde::{Deserialize, Serialize};

/// Supported storage backend types.
///
/// Serialized as lowercase strings (e.g., `"local"`, `"s3"`) in both the
/// database and IPC JSON payloads, via `#[serde(rename_all = "lowercase")]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DiskType {
    /// Local filesystem directory.
    Local,
    /// Amazon S3 (or S3-compatible) bucket.
    S3,
    /// Azure Blob Storage container.
    Azure,
    /// SFTP (SSH File Transfer Protocol) server.
    Sftp,
    /// FTP/FTPS server.
    Ftp,
    /// Google Cloud Storage bucket.
    Gcs,
}

/// Configuration for a storage profile ("disk").
///
/// Each disk has a unique ID, a human-readable name, a backend type, and
/// a free-form JSON config object whose schema depends on the backend type.
///
/// ## Config field schemas by backend type
///
/// - **Local**: `{ "root": "/absolute/path" }`
/// - **S3**: `{ "bucket": "...", "region": "...", "access_key_id": "...", "secret_access_key": "..." }`
/// - **Azure**: `{ "account": "...", "access_key": "...", "container": "..." }`
/// - **SFTP**: `{ "host": "...", "port": "22", "username": "...", "password": "..." }` or
///   `{ "host": "...", "port": "22", "username": "...", "key_path": "/path/to/key" }`
/// - **FTP**: `{ "host": "...", "port": "21", "username": "...", "password": "...", "tls": "true"|"false" }`
/// - **GCS**: `{ "bucket": "...", "credentials_json": "..." }`
///
/// Note that `port` and `tls` are stored as strings (not numbers/bools) because
/// the frontend sends form values as strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskConfig {
    /// Unique identifier (UUID v4 string).
    pub id: String,
    /// User-chosen display name for this disk.
    pub name: String,
    /// Which storage backend to use.
    pub disk_type: DiskType,
    /// Backend-specific configuration as a free-form JSON object.
    /// Credentials are stored here but redacted before IPC transmission.
    pub config: serde_json::Value,
    /// ISO 8601 timestamp of when this disk was created.
    pub created_at: String,
}
