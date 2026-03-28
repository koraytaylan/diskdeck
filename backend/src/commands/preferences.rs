//! # User preferences commands
//!
//! Simple key-value preference storage backed by the SQLCipher database.
//! Used by the frontend to persist user settings such as theme, layout
//! preferences, sort order, and other UI state.
//!
//! ## Design
//!
//! Preferences are stored as string key-value pairs in the `preferences`
//! table. No schema is imposed on the values — the frontend is responsible
//! for serializing/deserializing complex values (e.g., JSON-encoded objects).
//!
//! ## Validation
//!
//! - Key length is capped at 255 characters.
//! - Value length is capped at 10,000 characters.
//!
//! These limits prevent accidental misuse (e.g., storing large blobs as
//! preferences) while being generous enough for any realistic setting.

use tauri::State;

use crate::error::DiskDeckError;
use crate::state::AppState;

/// Core logic for getting a preference, testable without Tauri State.
pub(crate) fn get_preference_inner(
    state: &AppState,
    key: &str,
) -> Result<Option<String>, DiskDeckError> {
    if key.len() > 255 {
        return Err(DiskDeckError::Storage("Preference key too long".into()));
    }
    state.store.get_preference(key)
}

/// Core logic for setting a preference, testable without Tauri State.
pub(crate) fn set_preference_inner(
    state: &AppState,
    key: &str,
    value: &str,
) -> Result<(), DiskDeckError> {
    if key.len() > 255 {
        return Err(DiskDeckError::Storage("Preference key too long".into()));
    }
    if value.len() > 10_000 {
        return Err(DiskDeckError::Storage("Preference value too long".into()));
    }
    state.store.set_preference(key, value)
}

/// Retrieves a preference value by key.
///
/// Returns `Ok(None)` if the key does not exist (not an error).
///
/// # Errors
///
/// - Key longer than 255 characters.
/// - Database read failure.
#[tauri::command]
pub async fn get_preference(
    state: State<'_, AppState>,
    key: String,
) -> Result<Option<String>, DiskDeckError> {
    get_preference_inner(&state, &key)
}

/// Sets a preference value, creating or overwriting the key.
///
/// # Errors
///
/// - Key longer than 255 characters.
/// - Value longer than 10,000 characters.
/// - Database write failure.
#[tauri::command]
pub async fn set_preference(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), DiskDeckError> {
    set_preference_inner(&state, &key, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DiskStore;
    use crate::state::AppState;

    fn test_state() -> AppState {
        AppState::new(DiskStore::new_in_memory().unwrap())
    }

    #[test]
    fn preference_roundtrip() {
        let store = DiskStore::new_in_memory().unwrap();
        store.set_preference("key", "value").unwrap();
        assert_eq!(
            store.get_preference("key").unwrap(),
            Some("value".to_string())
        );
    }

    #[test]
    fn preference_missing_returns_none() {
        let store = DiskStore::new_in_memory().unwrap();
        assert_eq!(store.get_preference("missing").unwrap(), None);
    }

    #[test]
    fn preference_overwrite() {
        let store = DiskStore::new_in_memory().unwrap();
        store.set_preference("key", "v1").unwrap();
        store.set_preference("key", "v2").unwrap();
        assert_eq!(
            store.get_preference("key").unwrap(),
            Some("v2".to_string())
        );
    }

    // ---- get_preference_inner tests ----

    #[test]
    fn get_preference_inner_returns_value() {
        let state = test_state();
        state.store.set_preference("theme", "dark").unwrap();
        let result = get_preference_inner(&state, "theme").unwrap();
        assert_eq!(result, Some("dark".to_string()));
    }

    #[test]
    fn get_preference_inner_missing_returns_none() {
        let state = test_state();
        let result = get_preference_inner(&state, "missing").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn get_preference_inner_key_too_long() {
        let state = test_state();
        let long_key = "x".repeat(256);
        let result = get_preference_inner(&state, &long_key);
        assert!(result.is_err());
    }

    #[test]
    fn get_preference_inner_key_at_limit_ok() {
        let state = test_state();
        let key = "x".repeat(255);
        let result = get_preference_inner(&state, &key);
        assert!(result.is_ok());
    }

    // ---- set_preference_inner tests ----

    #[test]
    fn set_preference_inner_stores_value() {
        let state = test_state();
        set_preference_inner(&state, "key", "val").unwrap();
        assert_eq!(
            get_preference_inner(&state, "key").unwrap(),
            Some("val".to_string())
        );
    }

    #[test]
    fn set_preference_inner_key_too_long() {
        let state = test_state();
        let long_key = "x".repeat(256);
        let result = set_preference_inner(&state, &long_key, "val");
        assert!(result.is_err());
    }

    #[test]
    fn set_preference_inner_value_too_long() {
        let state = test_state();
        let long_val = "x".repeat(10_001);
        let result = set_preference_inner(&state, "key", &long_val);
        assert!(result.is_err());
    }

    #[test]
    fn set_preference_inner_value_at_limit_ok() {
        let state = test_state();
        let val = "x".repeat(10_000);
        let result = set_preference_inner(&state, "key", &val);
        assert!(result.is_ok());
    }

    #[test]
    fn set_preference_inner_overwrites() {
        let state = test_state();
        set_preference_inner(&state, "key", "v1").unwrap();
        set_preference_inner(&state, "key", "v2").unwrap();
        assert_eq!(
            get_preference_inner(&state, "key").unwrap(),
            Some("v2".to_string())
        );
    }
}
