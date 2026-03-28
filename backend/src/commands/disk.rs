//! # Disk CRUD commands and backend factory
//!
//! This module handles creating, listing, updating, and deleting disk
//! configurations. It also contains the **backend factory** — the logic that
//! instantiates the correct [`StorageBackend`](crate::storage::StorageBackend)
//! implementation from a [`DiskConfig`].
//!
//! ## Credential storage
//!
//! Credentials (passwords, secret keys) are stored alongside
//! the rest of the disk config in the `config` JSON column of the
//! SQLCipher-encrypted database. The database encryption key is the only
//! secret held in the OS keychain (one keychain access on startup, zero
//! when using disks). This eliminates per-disk keychain prompts entirely.
//!
//! ## Backend factory design
//!
//! There are two factory functions:
//!
//! - [`backend_from_config`] — The full async version that handles all backend
//!   types including S3. Used in production when creating/updating disks and when
//!   backends are constructed lazily via `get_backend` on first access.
//! - [`backend_from_config_sync`] — Handles backends that can be constructed
//!   without async (Local, GCS, Azure, SFTP, FTP). Returns `Ok(None)` for backends
//!   that require async initialization (S3). Used only in tests where a synchronous
//!   context is convenient.
//!
//! ## Credential redaction
//!
//! The [`redact_config`] function sanitizes disk configs before sending them
//! over IPC to the frontend. Sensitive fields (`secret_access_key`, `access_key`,
//! `password`, `credentials_json`) are replaced with empty strings so that
//! credentials are never exposed to the renderer process.
//!
//! ## Validation
//!
//! `create_disk` and `update_disk` validate:
//! - Disk name is not empty and not longer than 255 characters.
//! - The backend can actually be constructed from the provided config (this
//!   serves as a connectivity test for remote backends).

use std::sync::Arc;

use tauri::State;

use crate::error::DiskDeckError;
use crate::models::disk::{DiskConfig, DiskType};
use crate::state::AppState;
use crate::storage::azure::AzureBackend;
use crate::storage::ftp::FtpBackend;
use crate::storage::gcs::GcsBackend;
use crate::storage::local::LocalBackend;
use crate::storage::s3::S3Backend;
use crate::storage::sftp::SftpBackend;

/// Redacts sensitive fields from a DiskConfig before sending over IPC.
///
/// Replaces the values of known credential keys with empty strings.
/// The keys are checked in the top-level JSON object of `config`.
///
/// # Redacted keys
///
/// - `secret_access_key` (AWS S3)
/// - `access_key` (Azure)
/// - `password` (SFTP, FTP)
/// - `credentials_json` (GCS)
pub(crate) fn redact_config(disk: &DiskConfig) -> DiskConfig {
    let mut redacted = disk.clone();
    let sensitive_keys = [
        "secret_access_key",
        "access_key",
        "password",
        "credentials_json",
        "client_secret",
        "access_token",
        "refresh_token",
    ];
    if let serde_json::Value::Object(ref mut map) = redacted.config {
        for key in &sensitive_keys {
            if map.contains_key(*key) {
                map.insert((*key).to_string(), serde_json::Value::String(String::new()));
            }
        }
    }
    redacted
}

/// Extracts a required string field from a JSON config object.
///
/// # Errors
///
/// Returns `DiskDeckError::Storage` if the key is missing or not a string.
fn cfg_str<'a>(config: &'a serde_json::Value, key: &str) -> Result<&'a str, DiskDeckError> {
    config[key]
        .as_str()
        .ok_or_else(|| DiskDeckError::Storage(format!("Missing '{}' in config", key)))
}

/// Creates a storage backend synchronously (for backends that do not need async init).
///
/// Returns:
/// - `Ok(Some(backend))` for sync-constructible backends (Local, Azure, SFTP, FTP).
/// - `Ok(None)` for backends that require async initialization (S3).
///
/// This split exists because app startup restores backends before the Tokio runtime
/// is fully available. S3 backends are deferred to a spawned async task.
///
/// # Errors
///
/// Returns `DiskDeckError` if the config is missing required fields or if the
/// backend constructor fails (e.g., invalid Azure credentials).
#[cfg(test)]
pub fn backend_from_config_sync(
    disk: &DiskConfig,
) -> Result<Option<Arc<dyn crate::storage::StorageBackend>>, DiskDeckError> {
    let config = &disk.config;
    match &disk.disk_type {
        DiskType::Local => {
            let root = cfg_str(&config, "root")?;
            Ok(Some(Arc::new(LocalBackend::new(root))))
        }
        DiskType::S3 => Ok(None), // requires async init
        DiskType::Gcs => {
            let bucket = cfg_str(&config, "bucket")?;
            let credentials_json = cfg_str(&config, "credentials_json")?;
            Ok(Some(Arc::new(GcsBackend::new(bucket, credentials_json)?)))
        }
        DiskType::Azure => {
            let account = cfg_str(&config, "account")?;
            let access_key = cfg_str(&config, "access_key")?;
            let container = cfg_str(&config, "container")?;
            Ok(Some(Arc::new(AzureBackend::new(
                account,
                access_key,
                container,
            )?)))
        }
        DiskType::Sftp => {
            let host = cfg_str(&config, "host")?;
            // Port defaults to 22 for SFTP; stored as string in config JSON
            let port: u16 = config["port"]
                .as_str()
                .unwrap_or("22")
                .parse()
                .unwrap_or(22);
            let username = cfg_str(&config, "username")?;
            let key_path = config["key_path"].as_str().map(|s| s.to_string());
            // Password is optional when key_path is provided
            let password = cfg_str(&config, "password").unwrap_or("");
            Ok(Some(Arc::new(SftpBackend::new(
                host, port, username, password, key_path.as_deref(),
            ))))
        }
        DiskType::Ftp => {
            let host = cfg_str(&config, "host")?;
            // Port defaults to 21 for FTP; stored as string in config JSON
            let port: u16 = config["port"]
                .as_str()
                .unwrap_or("21")
                .parse()
                .unwrap_or(21);
            let username = cfg_str(&config, "username")?;
            let password = cfg_str(&config, "password")?;
            let tls = config["tls"].as_str() == Some("true");
            Ok(Some(Arc::new(FtpBackend::new(
                host, port, username, password, tls,
            ))))
        }
    }
}

/// Creates a storage backend from a disk config (async, handles all types).
///
/// This is the primary factory used at runtime when creating or updating disks.
/// Unlike [`backend_from_config_sync`], it can construct S3 backends which
/// require async AWS SDK config loading.
///
/// # Errors
///
/// Returns `DiskDeckError` if config fields are missing or backend construction
/// fails (e.g., invalid credentials, unreachable S3 endpoint).
pub async fn backend_from_config(
    disk: &DiskConfig,
) -> Result<Arc<dyn crate::storage::StorageBackend>, DiskDeckError> {
    let config = &disk.config;
    match &disk.disk_type {
        DiskType::Local => {
            let root = cfg_str(config, "root")?;
            Ok(Arc::new(LocalBackend::new(root)))
        }
        DiskType::S3 => {
            let bucket = cfg_str(config, "bucket")?;
            let region = cfg_str(config, "region")?;
            let access_key_id = cfg_str(config, "access_key_id")?;
            let secret_access_key = cfg_str(config, "secret_access_key")?;
            Ok(Arc::new(
                S3Backend::from_credentials(bucket, region, access_key_id, secret_access_key)
                    .await?,
            ))
        }
        DiskType::Gcs => {
            let bucket = cfg_str(config, "bucket")?;
            let credentials_json = cfg_str(config, "credentials_json")?;
            Ok(Arc::new(GcsBackend::new(bucket, credentials_json)?))
        }
        DiskType::Azure => {
            let account = cfg_str(config, "account")?;
            let access_key = cfg_str(config, "access_key")?;
            let container = cfg_str(config, "container")?;
            Ok(Arc::new(AzureBackend::new(account, access_key, container)?))
        }
        DiskType::Sftp => {
            let host = cfg_str(config, "host")?;
            let port: u16 = config["port"]
                .as_str()
                .unwrap_or("22")
                .parse()
                .unwrap_or(22);
            let username = cfg_str(config, "username")?;
            let key_path = config["key_path"].as_str().map(|s| s.to_string());
            // Password is optional when key_path is provided
            let password = cfg_str(config, "password").unwrap_or("");
            Ok(Arc::new(SftpBackend::new(host, port, username, password, key_path.as_deref())))
        }
        DiskType::Ftp => {
            let host = cfg_str(config, "host")?;
            let port: u16 = config["port"]
                .as_str()
                .unwrap_or("21")
                .parse()
                .unwrap_or(21);
            let username = cfg_str(config, "username")?;
            let password = cfg_str(config, "password")?;
            let tls = config["tls"].as_str() == Some("true");
            Ok(Arc::new(FtpBackend::new(host, port, username, password, tls)))
        }
    }
}

/// Validates a disk name: must be non-empty and at most 255 characters.
/// Returns the trimmed name on success.
pub(crate) fn validate_disk_name(name: &str) -> Result<String, DiskDeckError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(DiskDeckError::Storage("Disk name cannot be empty".into()));
    }
    if name.len() > 255 {
        return Err(DiskDeckError::Storage("Disk name too long (max 255 chars)".into()));
    }
    Ok(name)
}

/// Core logic for deleting a disk, testable without Tauri State.
///
/// Removes the disk from the backends map, the in-memory disk list, the
/// persistent database, and the FTS5 search index. Credentials are deleted
/// along with the config since they live in the same encrypted database row.
pub(crate) async fn delete_disk_inner(
    state: &AppState,
    disk_id: &str,
) -> Result<(), DiskDeckError> {
    state.backends.write().await.remove(disk_id);
    let mut disks = state.disks.write().await;
    disks.retain(|d| d.id != disk_id);
    state.store.save(&disks)?;
    let _ = state.store.clear_index(disk_id);
    Ok(())
}

/// Lists all disk configurations with credentials redacted.
///
/// Returns the full list of disks from the in-memory state. Sensitive config
/// fields are replaced with empty strings before serialization.
#[tauri::command]
pub async fn list_disks(state: State<'_, AppState>) -> Result<Vec<DiskConfig>, DiskDeckError> {
    let disks = state.disks.read().await;
    Ok(disks.iter().map(redact_config).collect())
}

/// Creates a new disk configuration and its storage backend.
///
/// Flow:
/// 1. Validate the disk name.
/// 2. Generate a UUID for the new disk.
/// 3. Attempt to construct the backend (serves as a connectivity test).
/// 4. Persist the full config (with credentials) to the encrypted database.
/// 5. Return the redacted config to the frontend.
///
/// # Errors
///
/// - Empty or too-long disk name.
/// - Missing config fields.
/// - Backend construction failure (e.g., unreachable server).
/// - Database write failure.
#[tauri::command]
pub async fn create_disk(
    state: State<'_, AppState>,
    name: String,
    disk_type: DiskType,
    config: serde_json::Value,
) -> Result<DiskConfig, DiskDeckError> {
    let name = validate_disk_name(&name)?;

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let disk = DiskConfig {
        id: id.clone(),
        name,
        disk_type,
        config,
        created_at: now,
    };

    // Validate connectivity before persisting
    let backend = backend_from_config(&disk).await?;

    state.backends.write().await.insert(id.clone(), backend);
    let mut disks = state.disks.write().await;
    disks.push(disk.clone());
    state.store.save(&disks)?;

    Ok(redact_config(&disk))
}

/// Updates an existing disk's name and configuration.
///
/// The disk type cannot be changed (only name and config). The backend is
/// rebuilt with the new config to validate connectivity. The full config
/// (with credentials) is saved to the encrypted database.
///
/// # Errors
///
/// - Disk ID not found.
/// - Invalid name or config.
/// - Backend construction failure.
/// - Database write failure.
#[tauri::command]
pub async fn update_disk(
    state: State<'_, AppState>,
    disk_id: String,
    name: String,
    config: serde_json::Value,
) -> Result<DiskConfig, DiskDeckError> {
    let name = validate_disk_name(&name)?;

    let mut disks = state.disks.write().await;
    let disk = disks
        .iter_mut()
        .find(|d| d.id == disk_id)
        .ok_or_else(|| DiskDeckError::Storage(format!("Disk '{}' not found", disk_id)))?;

    // Build a temporary disk with the full config to validate connectivity
    let temp_disk = DiskConfig {
        id: disk_id.clone(),
        name: name.clone(),
        disk_type: disk.disk_type.clone(),
        config: config.clone(),
        created_at: disk.created_at.clone(),
    };
    let backend = backend_from_config(&temp_disk).await?;

    disk.name = name;
    disk.config = config;

    state.backends.write().await.insert(disk_id, backend);

    let updated = disk.clone();
    state.store.save(&disks)?;

    Ok(redact_config(&updated))
}

/// Deletes a disk configuration, its backend, and its search index.
///
/// Removes the disk from:
/// 1. The backends map (drops the `Arc<dyn StorageBackend>`).
/// 2. The in-memory disks list.
/// 3. The persistent database.
/// 4. The FTS5 search index.
#[tauri::command]
pub async fn delete_disk(
    state: State<'_, AppState>,
    disk_id: String,
) -> Result<(), DiskDeckError> {
    delete_disk_inner(&state, &disk_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DiskStore;
    use crate::storage::memory::MemoryBackend;

    /// Generates a PEM-encoded RSA private key for testing.
    fn test_rsa_pem() -> String {
        use rsa::pkcs1::EncodeRsaPrivateKey;
        use rsa::RsaPrivateKey;
        let mut rng = rand::thread_rng();
        let key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        key.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
            .unwrap()
            .to_string()
    }

    fn test_state() -> AppState {
        AppState::new(DiskStore::new_in_memory().unwrap())
    }

    // ---- validate_disk_name tests ----

    #[test]
    fn validate_disk_name_ok() {
        assert_eq!(validate_disk_name("My Disk").unwrap(), "My Disk");
    }

    #[test]
    fn validate_disk_name_trims() {
        assert_eq!(validate_disk_name("  My Disk  ").unwrap(), "My Disk");
    }

    #[test]
    fn validate_disk_name_empty() {
        assert!(validate_disk_name("").is_err());
    }

    #[test]
    fn validate_disk_name_whitespace_only() {
        assert!(validate_disk_name("   ").is_err());
    }

    #[test]
    fn validate_disk_name_too_long() {
        let long = "x".repeat(256);
        assert!(validate_disk_name(&long).is_err());
    }

    #[test]
    fn validate_disk_name_at_limit() {
        let name = "x".repeat(255);
        assert!(validate_disk_name(&name).is_ok());
    }

    // ---- delete_disk_inner tests ----

    #[tokio::test]
    async fn delete_disk_inner_removes_backend_and_disk() {
        let state = test_state();
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Test".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({"root": "/tmp"}),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        state.disks.write().await.push(disk);
        let backend = std::sync::Arc::new(MemoryBackend::new());
        let backend_dyn: std::sync::Arc<dyn crate::storage::StorageBackend> = backend;
        state.backends.write().await.insert("d1".into(), backend_dyn);
        state.store.save(&*state.disks.read().await).unwrap();

        delete_disk_inner(&state, "d1").await.unwrap();

        assert!(state.backends.read().await.get("d1").is_none());
        assert!(state.disks.read().await.is_empty());
    }

    #[tokio::test]
    async fn delete_disk_inner_clears_index() {
        let state = test_state();
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Test".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({"root": "/tmp"}),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        state.disks.write().await.push(disk);
        state.store.save(&*state.disks.read().await).unwrap();
        state.store.index_entry("d1", "/a.txt", "a.txt", false, 10, None).unwrap();

        delete_disk_inner(&state, "d1").await.unwrap();

        let results = state.store.search_index("a", None, 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn redact_config_blanks_sensitive_keys() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Test".into(),
            disk_type: DiskType::S3,
            config: serde_json::json!({
                "bucket": "my-bucket",
                "region": "us-east-1",
                "access_key_id": "AKIA...",
                "secret_access_key": "secret123"
            }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let redacted = redact_config(&disk);
        assert_eq!(redacted.config["bucket"], "my-bucket");
        assert_eq!(redacted.config["region"], "us-east-1");
        assert_eq!(redacted.config["access_key_id"], "AKIA...");
        assert_eq!(redacted.config["secret_access_key"], "");
    }

    #[test]
    fn redact_config_blanks_password() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "FTP".into(),
            disk_type: DiskType::Ftp,
            config: serde_json::json!({ "host": "example.com", "username": "user", "password": "secret" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let redacted = redact_config(&disk);
        assert_eq!(redacted.config["host"], "example.com");
        assert_eq!(redacted.config["username"], "user");
        assert_eq!(redacted.config["password"], "");
    }

    #[test]
    fn redact_config_leaves_local_unchanged() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Local".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({ "root": "/home/user" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let redacted = redact_config(&disk);
        assert_eq!(redacted.config["root"], "/home/user");
    }

    #[test]
    fn cfg_str_extracts_field() {
        let config = serde_json::json!({ "host": "example.com" });
        assert_eq!(cfg_str(&config, "host").unwrap(), "example.com");
    }

    #[test]
    fn cfg_str_missing_field_errors() {
        let config = serde_json::json!({ "host": "example.com" });
        assert!(cfg_str(&config, "port").is_err());
    }

    #[test]
    fn cfg_str_non_string_field_errors() {
        let config = serde_json::json!({ "port": 22 });
        assert!(cfg_str(&config, "port").is_err());
    }

    #[test]
    fn backend_from_config_sync_local() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Local".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({ "root": "/tmp" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config_sync(&disk);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn backend_from_config_sync_s3_returns_none() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "S3".into(),
            disk_type: DiskType::S3,
            config: serde_json::json!({ "bucket": "b", "region": "r", "access_key_id": "a", "secret_access_key": "s" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config_sync(&disk).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn backend_from_config_sync_missing_root_errors() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Local".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({}),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert!(backend_from_config_sync(&disk).is_err());
    }

    #[test]
    fn backend_from_config_sync_ftp() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "FTP".into(),
            disk_type: DiskType::Ftp,
            config: serde_json::json!({ "host": "example.com", "port": "21", "username": "user", "password": "pass", "tls": "false" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config_sync(&disk);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn backend_from_config_sync_sftp() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "SFTP".into(),
            disk_type: DiskType::Sftp,
            config: serde_json::json!({ "host": "example.com", "port": "22", "username": "user", "password": "pass" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config_sync(&disk);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn backend_from_config_sync_ftp_missing_host_errors() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "FTP".into(),
            disk_type: DiskType::Ftp,
            config: serde_json::json!({ "username": "user", "password": "pass" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert!(backend_from_config_sync(&disk).is_err());
    }

    #[test]
    fn backend_from_config_sync_ftp_default_port() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "FTP".into(),
            disk_type: DiskType::Ftp,
            config: serde_json::json!({ "host": "example.com", "username": "user", "password": "pass" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config_sync(&disk);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn backend_from_config_sync_azure() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Azure".into(),
            disk_type: DiskType::Azure,
            config: serde_json::json!({ "account": "myaccount", "access_key": "dGVzdGtleQ==", "container": "mycontainer" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config_sync(&disk);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn backend_from_config_sync_azure_missing_field_errors() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Azure".into(),
            disk_type: DiskType::Azure,
            config: serde_json::json!({ "account": "myaccount" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert!(backend_from_config_sync(&disk).is_err());
    }

    #[test]
    fn backend_from_config_sync_sftp_missing_field_errors() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "SFTP".into(),
            disk_type: DiskType::Sftp,
            config: serde_json::json!({ "host": "example.com" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert!(backend_from_config_sync(&disk).is_err());
    }

    #[test]
    fn backend_from_config_sync_sftp_default_port() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "SFTP".into(),
            disk_type: DiskType::Sftp,
            config: serde_json::json!({ "host": "example.com", "username": "user", "password": "pass" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config_sync(&disk);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn backend_from_config_sync_ftp_tls() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "FTP".into(),
            disk_type: DiskType::Ftp,
            config: serde_json::json!({ "host": "example.com", "port": "21", "username": "user", "password": "pass", "tls": "true" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config_sync(&disk);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn redact_config_blanks_azure_access_key() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Azure".into(),
            disk_type: DiskType::Azure,
            config: serde_json::json!({ "account": "acc", "access_key": "secret", "container": "cont" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let redacted = redact_config(&disk);
        assert_eq!(redacted.config["account"], "acc");
        assert_eq!(redacted.config["access_key"], "");
        assert_eq!(redacted.config["container"], "cont");
    }

    #[tokio::test]
    async fn backend_from_config_async_local() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Local".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({ "root": "/tmp" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config(&disk).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn backend_from_config_async_local_missing_root_errors() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Local".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({}),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert!(backend_from_config(&disk).await.is_err());
    }

    #[tokio::test]
    async fn backend_from_config_async_azure() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Azure".into(),
            disk_type: DiskType::Azure,
            config: serde_json::json!({ "account": "myaccount", "access_key": "dGVzdGtleQ==", "container": "mycontainer" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config(&disk).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn backend_from_config_async_sftp() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "SFTP".into(),
            disk_type: DiskType::Sftp,
            config: serde_json::json!({ "host": "example.com", "port": "22", "username": "user", "password": "pass" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config(&disk).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn backend_from_config_async_ftp() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "FTP".into(),
            disk_type: DiskType::Ftp,
            config: serde_json::json!({ "host": "example.com", "port": "21", "username": "user", "password": "pass", "tls": "false" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config(&disk).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn backend_from_config_async_s3() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "S3".into(),
            disk_type: DiskType::S3,
            config: serde_json::json!({ "bucket": "b", "region": "us-east-1", "access_key_id": "AKIA", "secret_access_key": "secret" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config(&disk).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn backend_from_config_async_s3_missing_field_errors() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "S3".into(),
            disk_type: DiskType::S3,
            config: serde_json::json!({ "bucket": "b" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert!(backend_from_config(&disk).await.is_err());
    }

    #[test]
    fn backend_from_config_sync_sftp_with_key_path() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "SFTP Key".into(),
            disk_type: DiskType::Sftp,
            config: serde_json::json!({
                "host": "example.com",
                "port": "22",
                "username": "user",
                "key_path": "/home/user/.ssh/id_rsa"
            }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config_sync(&disk);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[tokio::test]
    async fn backend_from_config_async_sftp_with_key_path() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "SFTP Key".into(),
            disk_type: DiskType::Sftp,
            config: serde_json::json!({
                "host": "example.com",
                "port": "22",
                "username": "user",
                "key_path": "/home/user/.ssh/id_rsa"
            }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config(&disk).await;
        assert!(result.is_ok());
    }

    #[test]
    fn redact_config_blanks_gcs_credentials_json() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "GCS".into(),
            disk_type: DiskType::Gcs,
            config: serde_json::json!({
                "bucket": "my-bucket",
                "credentials_json": "{\"client_email\":\"test@proj.iam.gserviceaccount.com\",\"private_key\":\"secret\"}"
            }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let redacted = redact_config(&disk);
        assert_eq!(redacted.config["bucket"], "my-bucket");
        assert_eq!(redacted.config["credentials_json"], "");
    }

    #[test]
    fn backend_from_config_sync_gcs() {
        let creds = serde_json::json!({
            "client_email": "test@project.iam.gserviceaccount.com",
            "private_key": test_rsa_pem()
        });
        let disk = DiskConfig {
            id: "d1".into(),
            name: "GCS".into(),
            disk_type: DiskType::Gcs,
            config: serde_json::json!({ "bucket": "my-bucket", "credentials_json": creds.to_string() }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config_sync(&disk);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn backend_from_config_sync_gcs_missing_bucket_errors() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "GCS".into(),
            disk_type: DiskType::Gcs,
            config: serde_json::json!({ "credentials_json": "{}" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert!(backend_from_config_sync(&disk).is_err());
    }

    #[test]
    fn backend_from_config_sync_gcs_missing_credentials_errors() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "GCS".into(),
            disk_type: DiskType::Gcs,
            config: serde_json::json!({ "bucket": "my-bucket" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert!(backend_from_config_sync(&disk).is_err());
    }

    #[test]
    fn backend_from_config_sync_gcs_invalid_credentials_errors() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "GCS".into(),
            disk_type: DiskType::Gcs,
            config: serde_json::json!({ "bucket": "my-bucket", "credentials_json": "not json" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert!(backend_from_config_sync(&disk).is_err());
    }

    #[tokio::test]
    async fn backend_from_config_async_gcs() {
        let creds = serde_json::json!({
            "client_email": "test@project.iam.gserviceaccount.com",
            "private_key": test_rsa_pem()
        });
        let disk = DiskConfig {
            id: "d1".into(),
            name: "GCS".into(),
            disk_type: DiskType::Gcs,
            config: serde_json::json!({ "bucket": "my-bucket", "credentials_json": creds.to_string() }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let result = backend_from_config(&disk).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn backend_from_config_async_gcs_missing_field_errors() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "GCS".into(),
            disk_type: DiskType::Gcs,
            config: serde_json::json!({ "bucket": "b" }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert!(backend_from_config(&disk).await.is_err());
    }

    #[test]
    fn redact_config_with_non_object_config() {
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Test".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!("not an object"),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let redacted = redact_config(&disk);
        assert_eq!(redacted.config, serde_json::json!("not an object"));
    }

}
