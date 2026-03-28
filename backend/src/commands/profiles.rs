//! # Profile export / import commands
//!
//! Provides two Tauri commands for sharing disk configurations between
//! DiskDeck instances or team members:
//!
//! - [`export_profiles`] — Serializes all disk configs to JSON with
//!   credentials stripped (passwords, secret keys replaced with empty strings).
//! - [`import_profiles`] — Deserializes disk configs from JSON, assigns fresh
//!   UUIDs and timestamps, and persists them. Credentials are **not** imported;
//!   the user must fill them in via the Edit Disk dialog.
//!
//! ## Security
//!
//! Exported profiles use the same [`redact_config`](super::disk::redact_config)
//! function as the IPC `list_disks` command, so no secrets ever leave the
//! backend in plaintext.

use tauri::State;

use crate::error::DiskDeckError;
use crate::models::disk::DiskConfig;
use crate::state::AppState;

use super::disk::redact_config;

/// Core logic for exporting profiles, testable without Tauri State.
pub(crate) async fn export_profiles_inner(
    state: &AppState,
) -> Result<String, DiskDeckError> {
    let disks = state.disks.read().await;
    let redacted: Vec<_> = disks.iter().map(redact_config).collect();
    serde_json::to_string_pretty(&redacted).map_err(DiskDeckError::Serialization)
}

/// Core logic for importing profiles, testable without Tauri State.
pub(crate) async fn import_profiles_inner(
    state: &AppState,
    json: String,
) -> Result<Vec<DiskConfig>, DiskDeckError> {
    let imported: Vec<DiskConfig> =
        serde_json::from_str(&json).map_err(DiskDeckError::Serialization)?;

    let mut created = Vec::new();
    let mut disks = state.disks.write().await;

    for mut disk in imported {
        disk.id = uuid::Uuid::new_v4().to_string();
        disk.created_at = chrono::Utc::now().to_rfc3339();
        disks.push(disk.clone());
        created.push(redact_config(&disk));
    }

    state.store.save(&disks)?;
    Ok(created)
}

/// Exports all disk configurations as pretty-printed JSON with credentials redacted.
///
/// The returned string can be saved to a file or copied to the clipboard for
/// sharing. Sensitive fields (passwords, secret keys) are replaced with empty
/// strings before serialization.
///
/// # Errors
///
/// Returns `DiskDeckError::Serialization` if JSON serialization fails (unlikely
/// since `DiskConfig` is always serializable).
#[tauri::command]
pub async fn export_profiles(
    state: State<'_, AppState>,
) -> Result<String, DiskDeckError> {
    export_profiles_inner(&state).await
}

/// Imports disk configurations from a JSON string.
///
/// Each imported disk receives a fresh UUID and creation timestamp. Credentials
/// are **not** restored from the import — the user must fill them in via the
/// Edit Disk dialog after import.
///
/// # Arguments
///
/// * `json` — A JSON string containing an array of `DiskConfig` objects
///   (typically produced by [`export_profiles`]).
///
/// # Returns
///
/// A `Vec<DiskConfig>` of the newly created disks (with redacted credentials),
/// so the frontend can update its disk list.
///
/// # Errors
///
/// - `DiskDeckError::Serialization` if the JSON is malformed or does not match
///   the `DiskConfig` schema.
/// - `DiskDeckError::Database` if persisting the new disks fails.
#[tauri::command]
pub async fn import_profiles(
    state: State<'_, AppState>,
    json: String,
) -> Result<Vec<DiskConfig>, DiskDeckError> {
    import_profiles_inner(&state, json).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DiskStore;
    use crate::models::disk::DiskType;

    fn test_state() -> AppState {
        AppState::new(DiskStore::new_in_memory().unwrap())
    }

    #[tokio::test]
    async fn export_returns_valid_json_with_redacted_credentials() {
        let state = test_state();
        let disk = DiskConfig {
            id: "d1".into(),
            name: "Test SFTP".into(),
            disk_type: DiskType::Sftp,
            config: serde_json::json!({
                "host": "example.com",
                "port": "22",
                "username": "user",
                "password": "secret"
            }),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        state.disks.write().await.push(disk);

        let json = export_profiles_inner(&state).await.unwrap();
        let parsed: Vec<DiskConfig> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "Test SFTP");
        // Password should be redacted
        assert_eq!(parsed[0].config["password"], "");
        // Non-sensitive fields preserved
        assert_eq!(parsed[0].config["host"], "example.com");
    }

    #[tokio::test]
    async fn import_creates_disks_with_new_uuids() {
        let state = test_state();
        let json = serde_json::to_string(&vec![DiskConfig {
            id: "old-id".into(),
            name: "Imported".into(),
            disk_type: DiskType::Local,
            config: serde_json::json!({ "root": "/tmp" }),
            created_at: "2020-01-01T00:00:00Z".into(),
        }])
        .unwrap();

        let result = import_profiles_inner(&state, json).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Imported");
        // UUID should be different from the original
        assert_ne!(result[0].id, "old-id");
        // created_at should be updated
        assert_ne!(result[0].created_at, "2020-01-01T00:00:00Z");
        // Disk should be persisted in state
        let disks = state.disks.read().await;
        assert_eq!(disks.len(), 1);
    }

    #[tokio::test]
    async fn import_invalid_json_returns_error() {
        let state = test_state();
        let result = import_profiles_inner(&state, "not valid json".into()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn export_empty_profiles_returns_empty_array() {
        let state = test_state();
        let json = export_profiles_inner(&state).await.unwrap();
        let parsed: Vec<DiskConfig> = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_empty());
    }

    #[tokio::test]
    async fn import_multiple_disks_assigns_unique_ids() {
        let state = test_state();
        let json = serde_json::to_string(&vec![
            DiskConfig {
                id: "same-id".into(),
                name: "Disk A".into(),
                disk_type: DiskType::Local,
                config: serde_json::json!({ "root": "/a" }),
                created_at: "2020-01-01T00:00:00Z".into(),
            },
            DiskConfig {
                id: "same-id".into(),
                name: "Disk B".into(),
                disk_type: DiskType::Local,
                config: serde_json::json!({ "root": "/b" }),
                created_at: "2020-01-01T00:00:00Z".into(),
            },
        ])
        .unwrap();

        let result = import_profiles_inner(&state, json).await.unwrap();
        assert_eq!(result.len(), 2);
        assert_ne!(result[0].id, result[1].id);
        assert_ne!(result[0].id, "same-id");
    }
}
