//! # Data models
//!
//! Shared data structures that are serialized across the Tauri IPC boundary.
//! All models derive `Serialize` and `Deserialize` so they can be passed
//! between the Rust backend and the TypeScript frontend as JSON.
//!
//! ## Submodules
//!
//! - [`disk`] — Disk configuration and backend type enum.
//! - [`entry`] — File/directory metadata returned by storage operations.
//! - [`search`] — Search query parameters and grouped results.
//! - [`job`] — Job tracking types for bulk file operations (copy, move, delete).
//! - [`bookmark`] — Pinned path bookmarks for quick-access navigation.
//! - [`diff`] — Directory comparison types for the sync/diff feature.

pub mod bookmark;
pub mod diff;
pub mod disk;
pub mod entry;
pub mod job;
pub mod search;
