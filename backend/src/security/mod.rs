//! # Security module
//!
//! Provides OS keychain integration for the database encryption key.
//!
//! Contains a single submodule:
//!
//! - [`keyring`] — Manages the SQLCipher database encryption key stored
//!   in the platform's native credential store (macOS Keychain, Windows
//!   Credential Manager, Linux Secret Service). Retrieved once on app
//!   startup; all disk credentials live inside the encrypted database.

pub mod keyring;
