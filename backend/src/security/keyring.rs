//! # OS keychain integration
//!
//! Single responsibility: manage the **database encryption key**.
//!
//! A 32-byte random key is stored under `db-encryption-key` in the OS
//! keychain. It is retrieved once on app startup to open the SQLCipher
//! database. All disk credentials live inside that encrypted database,
//! so no per-disk keychain entries are needed.
//!
//! ## Platform backends
//!
//! The `keyring` crate automatically selects the appropriate backend:
//!
//! - **macOS**: Keychain Services (`Security.framework`)
//! - **Windows**: Windows Credential Manager
//! - **Linux**: Secret Service API (via D-Bus, e.g., GNOME Keyring, KWallet)

use crate::error::DiskDeckError;

/// Service identifier for all keychain entries.
const SERVICE: &str = "com.diskdeck.app";

/// Retrieves the database encryption key from the OS keychain,
/// or generates and stores a new one if none exists.
///
/// The key is a 64-character hex string representing 32 bytes of
/// cryptographically random data, suitable for use as a SQLCipher key.
pub fn get_or_create_db_key() -> Result<String, DiskDeckError> {
    let entry = keyring::Entry::new(SERVICE, "db-encryption-key")
        .map_err(|e| DiskDeckError::Storage(format!("keyring init: {e}")))?;

    match entry.get_password() {
        Ok(key) => Ok(key),
        Err(keyring::Error::NoEntry) => {
            let key = generate_hex_key();
            entry
                .set_password(&key)
                .map_err(|e| DiskDeckError::Storage(format!("keyring set: {e}")))?;
            Ok(key)
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("cancel") || msg.contains("denied") || msg.contains("user") {
                Err(DiskDeckError::Storage("keychain_denied".into()))
            } else {
                Err(DiskDeckError::Storage(format!("keyring get: {e}")))
            }
        }
    }
}

/// Generates a 64-character hex string from 32 cryptographically random bytes.
fn generate_hex_key() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::thread_rng().gen();
    hex::encode(bytes)
}
