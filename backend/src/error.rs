//! # Error types for the DiskDeck backend
//!
//! Defines [`DiskDeckError`], the unified error enum used across the entire backend.
//! Every Tauri command returns `Result<T, DiskDeckError>`.
//!
//! ## Serialization strategy
//!
//! Tauri serializes command errors via `serde::Serialize` and sends them to the
//! frontend as strings. The custom `Serialize` implementation here deliberately
//! **sanitizes** error messages before they cross the IPC boundary:
//!
//! - Internal details (file paths, SQL errors, stack traces) are logged server-side
//!   at `debug` level but never exposed to the frontend.
//! - The frontend receives a short, user-friendly description (e.g., "Permission
//!   denied", "Database error").
//!
//! This prevents leaking sensitive filesystem or infrastructure details to the
//! renderer process.
//!
//! ## Conversion traits
//!
//! - `From<std::io::Error>` — wraps OS-level I/O errors.
//! - `From<serde_json::Error>` — wraps JSON serialization/deserialization failures.
//! - `From<rusqlite::Error>` — wraps SQLite/SQLCipher errors (stringified to avoid
//!   exposing the rusqlite type across the crate boundary).

use thiserror::Error;

/// Unified error type for all DiskDeck backend operations.
///
/// Variants are chosen to cover the major failure domains:
/// - **Io** — filesystem and network I/O errors.
/// - **Storage** — backend-specific errors (S3 API failures, SFTP disconnects, etc.).
/// - **NotFound** — a requested disk, file, or resource does not exist.
/// - **Serialization** — JSON parsing or encoding failed.
/// - **Database** — SQLite/SQLCipher errors (schema, query, lock poisoning).
#[derive(Debug, Error)]
pub enum DiskDeckError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Database error: {0}")]
    Database(String),
}

/// Converts a `rusqlite::Error` into [`DiskDeckError::Database`].
///
/// The original error is stringified rather than wrapped because `rusqlite::Error`
/// does not implement `serde::Serialize`, and we want a clean crate-level API.
impl From<rusqlite::Error> for DiskDeckError {
    fn from(e: rusqlite::Error) -> Self {
        DiskDeckError::Database(e.to_string())
    }
}

/// Custom serialization for IPC transport.
///
/// Instead of serializing the full enum structure, this impl emits a single string
/// with a sanitized, user-safe message. The full error (including internal details)
/// is logged at `debug` level for backend troubleshooting.
impl serde::Serialize for DiskDeckError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Log full error details server-side
        log::debug!("IPC error: {self:?}");
        // Return sanitized messages to frontend
        let message = match self {
            DiskDeckError::Io(e) => match e.kind() {
                std::io::ErrorKind::NotFound => "File or directory not found".to_string(),
                std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
                _ => "I/O error occurred".to_string(),
            },
            DiskDeckError::NotFound(msg) => msg.clone(),
            DiskDeckError::Storage(msg) => msg.clone(),
            DiskDeckError::Database(_) => "Database error".to_string(),
            DiskDeckError::Serialization(_) => "Data format error".to_string(),
        };
        serializer.serialize_str(&message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_not_found_serializes_safely() {
        let err = DiskDeckError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, "\"File or directory not found\"");
    }

    #[test]
    fn io_permission_denied_serializes_safely() {
        let err = DiskDeckError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "nope",
        ));
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, "\"Permission denied\"");
    }

    #[test]
    fn io_other_sanitized() {
        let err = DiskDeckError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "internal details",
        ));
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, "\"I/O error occurred\"");
    }

    #[test]
    fn database_error_sanitized() {
        let err = DiskDeckError::Database("sqlite error: table locked".into());
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, "\"Database error\"");
    }

    #[test]
    fn storage_error_passes_through() {
        let err = DiskDeckError::Storage("Disk name cannot be empty".into());
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, "\"Disk name cannot be empty\"");
    }

    #[test]
    fn not_found_passes_through() {
        let err = DiskDeckError::NotFound("Disk 'x' not found".into());
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, "\"Disk 'x' not found\"");
    }

    #[test]
    fn serialization_error_sanitized() {
        let err: Result<serde_json::Value, _> = serde_json::from_str("invalid json");
        let disk_err = DiskDeckError::Serialization(err.unwrap_err());
        let json = serde_json::to_string(&disk_err).unwrap();
        assert_eq!(json, "\"Data format error\"");
    }

    #[test]
    fn display_trait_storage() {
        let err = DiskDeckError::Storage("test".into());
        assert_eq!(err.to_string(), "Storage error: test");
    }

    #[test]
    fn display_trait_not_found() {
        let err = DiskDeckError::NotFound("missing".into());
        assert_eq!(err.to_string(), "Not found: missing");
    }

    #[test]
    fn display_trait_database() {
        let err = DiskDeckError::Database("db error".into());
        assert_eq!(err.to_string(), "Database error: db error");
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let err: DiskDeckError = io_err.into();
        assert!(matches!(err, DiskDeckError::Io(_)));
    }

    #[test]
    fn from_serde_error() {
        let serde_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
        let err: DiskDeckError = serde_err.into();
        assert!(matches!(err, DiskDeckError::Serialization(_)));
    }

    #[test]
    fn from_rusqlite_error() {
        let rusqlite_err = rusqlite::Error::QueryReturnedNoRows;
        let err: DiskDeckError = rusqlite_err.into();
        assert!(matches!(err, DiskDeckError::Database(_)));
    }

    #[test]
    fn display_trait_io() {
        let err = DiskDeckError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert!(err.to_string().contains("IO error"));
    }

    #[test]
    fn display_trait_serialization() {
        let serde_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
        let err = DiskDeckError::Serialization(serde_err);
        assert!(err.to_string().contains("Serialization error"));
    }
}
