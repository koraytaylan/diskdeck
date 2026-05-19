//! # Database layer (SQLCipher + FTS5)
//!
//! Provides the [`DiskStore`] struct, which is the single access point for all
//! persistent data: disk configurations (including credentials), user
//! preferences, bookmarks, and the full-text search index.
//!
//! ## SQLCipher encryption
//!
//! The database file (`diskdeck.db`) is encrypted with SQLCipher. The
//! encryption key is stored in the OS keychain (see
//! [`crate::security::keyring`]) and applied via `PRAGMA key` on every open.
//! Disk credentials (passwords, secret keys) are stored inside the encrypted
//! database as part of each disk's configuration JSON.
//!
//! ## Recovery from corrupt or unreadable databases
//!
//! If the database file exists but cannot be decrypted (wrong key or
//! corruption detected via failed `user_version` pragma or `Connection::open`),
//! a timestamped backup is created first:
//!
//! - `diskdeck.db.corrupt.<YYYYMMDD-HHMMSS>.bak` (raw byte copy of the original)
//! - `diskdeck.db.corrupt.<YYYYMMDD-HHMMSS>.bak.txt` (sidecar explaining the
//!   incident, timestamp, and how to attempt manual restore by renaming back)
//!
//! The backup is written to the **same directory** as the original. Only after
//! the backup copy (`.bak` file) succeeds is the corrupt `diskdeck.db` removed
//! and a fresh database created. The accompanying `.bak.txt` sidecar is
//! best-effort (written after the critical backup copy; a sidecar failure does
//! not prevent deletion of the original or recovery). This gives users and
//! support staff a post-mortem recovery path without changing the "start fresh
//! on irrecoverable key/DB" safety posture.
//!
//! If the backup copy fails for any reason (e.g. disk full), the original file
//! is left untouched and a clear `DiskDeckError` is returned instead of deleting.
//!
//! ## Schema migrations
//!
//! Migrations are tracked via `PRAGMA user_version`:
//!
//! - **Version 1**: Creates `disks` and `preferences` tables.
//! - **Version 2**: Creates `search_entries`, `search_index` (FTS5 virtual table),
//!   `index_meta`, and the triggers that keep the FTS5 index in sync.
//! - **Version 3**: Creates `bookmarks` table for pinned path bookmarks.
//!
//! Migrations are idempotent (`CREATE TABLE IF NOT EXISTS`) so re-running them
//! on an already-migrated database is safe.
//!
//! ## FTS5 full-text search index
//!
//! The search index is implemented as a **content-sync** FTS5 table:
//!
//! - `search_entries` is the **content table** storing the actual data (disk_id,
//!   path, name, is_dir, size, modified).
//! - `search_index` is the **FTS5 virtual table** indexing only the `name` column.
//! - Three triggers (`search_entries_ai`, `search_entries_ad`, `search_entries_au`)
//!   keep the FTS5 index in sync with the content table on insert, delete, and
//!   update.
//!
//! FTS5 queries use prefix matching (e.g., searching for "doc" matches
//! "document.pdf") via the `build_fts_query` function, which wraps each
//! search token in quotes and appends `*`.
//!
//! ## Index operations
//!
//! - [`DiskStore::index_entries_bulk`] — Replaces all entries for a disk in one
//!   transaction (used by the background indexer).
//! - [`DiskStore::index_entry`] — Upserts a single entry (used by file commands).
//! - [`DiskStore::remove_entry`] / [`DiskStore::remove_entries_under`] — Removes
//!   entries by exact path or path prefix.
//! - [`DiskStore::search_index`] — Queries the FTS5 index with optional disk
//!   scoping and a result limit.
//!
//! ## Concurrency
//!
//! `DiskStore` wraps the SQLite `Connection` in a `std::sync::Mutex` (not a
//! Tokio mutex) because SQLite operations are synchronous and very fast. The
//! lock is never held across `.await` points, so a standard mutex is correct
//! and avoids unnecessary async overhead.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::DiskDeckError;
use crate::models::bookmark::Bookmark;
use crate::models::disk::{DiskConfig, DiskType};

/// A file/folder entry in the search index.
///
/// This is the Rust-side representation of a row in the `search_entries` table.
/// It is used for both indexing (writing to the DB) and search results (reading
/// from the DB). Note: `disk_id` is often set to an empty string by the indexer
/// and filled in by the bulk-insert method.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    /// The disk this entry belongs to.
    pub disk_id: String,
    /// Virtual path (e.g., `/documents/report.pdf`).
    pub path: String,
    /// Filename or directory name.
    pub name: String,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// File size in bytes.
    pub size: u64,
    /// Last modification time as Unix timestamp. `None` if unavailable.
    pub modified: Option<i64>,
}

/// Metadata about the index state for a single disk.
///
/// Used by the frontend to show index status indicators and by the search
/// command to decide between FTS5 and live-search paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    /// The disk ID this metadata belongs to.
    pub disk_id: String,
    /// Current status: `"ready"`, `"indexing"`, or `"stale"`.
    pub status: String,
    /// Number of entries in the index for this disk.
    pub entry_count: i64,
    /// ISO 8601 timestamp of when indexing last completed successfully.
    /// `None` if the disk has never been fully indexed.
    pub last_indexed: Option<String>,
}

/// Persists disk configurations (without credentials), preferences, bookmarks,
/// and the search index in a plain SQLite database.
///
/// Sensitive credentials are stored separately in the OS keychain per disk
/// (see [`crate::security::keyring`]).
///
/// This is the only struct that directly interacts with the database. All
/// database access goes through methods on this struct.
pub struct DiskStore {
    /// The SQLite connection wrapped in a standard mutex.
    /// See module-level docs for why `std::sync::Mutex` is used over `tokio::sync::Mutex`.
    conn: Mutex<Connection>,
}

impl DiskStore {
    /// Acquires the database connection mutex.
    ///
    /// # Errors
    ///
    /// Returns `DiskDeckError::Database("database lock poisoned")` if a previous
    /// holder panicked while holding the lock. This should never happen in
    /// normal operation.
    fn conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, DiskDeckError> {
        self.conn
            .lock()
            .map_err(|_| DiskDeckError::Database("database lock poisoned".into()))
    }

    /// Opens (or creates) an encrypted SQLite database at `app_data_dir/diskdeck.db`.
    ///
    /// The database is encrypted with SQLCipher using the provided hex key.
    /// Creates the directory tree if it does not exist and runs all pending
    /// migrations.
    ///
    /// # Arguments
    ///
    /// * `app_data_dir` — The platform-specific app data directory.
    /// * `key` — Hex-encoded 32-byte SQLCipher encryption key from the OS keychain.
    ///
    /// # Errors
    ///
    /// Returns errors if directory creation, database opening, or migration fails.
    pub fn new(app_data_dir: &Path, key: &str) -> Result<Self, DiskDeckError> {
        std::fs::create_dir_all(app_data_dir)?;
        let db_path = app_data_dir.join("diskdeck.db");

        match Connection::open(&db_path) {
            Ok(conn) => {
                // Apply the encryption key
                if !key.is_empty() {
                    conn.pragma_update(None, "key", format!("x'{key}'"))?;
                }
                // Verify the database is readable
                match conn.pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0)) {
                    Ok(_) => Self::init(conn),
                    Err(e) => {
                        // Key mismatch or corrupt DB — backup first, then recreate
                        log::warn!(
                            "Database appears corrupt or key mismatch, starting fresh: {}",
                            e
                        );
                        drop(conn);
                        backup_and_remove_corrupt_db(
                            &db_path,
                            &format!(
                                "user_version pragma failed after key application (possible key mismatch or corruption): {}",
                                e
                            ),
                        )?;
                        let conn = Connection::open(&db_path)?;
                        if !key.is_empty() {
                            conn.pragma_update(None, "key", format!("x'{key}'"))?;
                        }
                        Self::init(conn)
                    }
                }
            }
            Err(e) => {
                // File cannot be opened at all — backup first (if exists), then recreate
                log::warn!("Database cannot be opened, starting fresh: {}", e);
                backup_and_remove_corrupt_db(
                    &db_path,
                    &format!(
                        "Connection::open failed (file missing, unreadable, or permissions issue): {}",
                        e
                    ),
                )?;
                let conn = Connection::open(&db_path)?;
                if !key.is_empty() {
                    conn.pragma_update(None, "key", format!("x'{key}'"))?;
                }
                Self::init(conn)
            }
        }
    }

    /// Creates an unencrypted in-memory database for testing.
    ///
    /// The database exists only for the lifetime of this `DiskStore` instance.
    #[cfg(test)]
    pub fn new_in_memory() -> Result<Self, DiskDeckError> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    /// Runs migrations and wraps the connection in a `DiskStore`.
    fn init(conn: Connection) -> Result<Self, DiskDeckError> {
        run_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Loads all disk configurations from the `disks` table.
    ///
    /// Deserializes the `disk_type` string and `config` JSON for each row.
    ///
    /// # Errors
    ///
    /// Returns errors if the database read fails or stored data is malformed.
    pub fn load(&self) -> Result<Vec<DiskConfig>, DiskDeckError> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT id, name, disk_type, config, created_at FROM disks")?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let disk_type_str: String = row.get(2)?;
            let config_str: String = row.get(3)?;
            let created_at: String = row.get(4)?;
            Ok((id, name, disk_type_str, config_str, created_at))
        })?;

        let mut disks = Vec::new();
        for row in rows {
            let (id, name, disk_type_str, config_str, created_at) = row?;
            // DiskType is serialized as a bare string (e.g., "local"), but serde
            // expects a quoted JSON string, so we wrap it in quotes for parsing.
            let disk_type: DiskType = serde_json::from_str(&format!("\"{disk_type_str}\""))
                .map_err(|e| DiskDeckError::Database(format!("invalid disk_type: {e}")))?;
            let config: serde_json::Value = serde_json::from_str(&config_str)
                .map_err(|e| DiskDeckError::Database(format!("invalid config JSON: {e}")))?;
            disks.push(DiskConfig {
                id,
                name,
                disk_type,
                config,
                created_at,
            });
        }
        Ok(disks)
    }

    /// Replaces all disk configurations in the database.
    ///
    /// Uses a transaction to atomically delete all existing rows and insert
    /// the new set. This "replace all" approach is simpler than diffing and
    /// is safe because the disk list is always small (typically <20 items).
    ///
    /// # Errors
    ///
    /// Returns errors if the transaction or any insert fails.
    pub fn save(&self, disks: &[DiskConfig]) -> Result<(), DiskDeckError> {
        let conn = self.conn()?;
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM disks", [])?;
        for disk in disks {
            let disk_type_str = serde_json::to_string(&disk.disk_type)
                .map_err(|e| DiskDeckError::Database(e.to_string()))?;
            // serde_json::to_string wraps enums in quotes; strip them for plain storage
            let disk_type_str = disk_type_str.trim_matches('"');
            let config_str = serde_json::to_string(&disk.config)
                .map_err(|e| DiskDeckError::Database(e.to_string()))?;
            tx.execute(
                "INSERT INTO disks (id, name, disk_type, config, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![disk.id, disk.name, disk_type_str, config_str, disk.created_at],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Gets a preference value by key.
    ///
    /// Returns `Ok(None)` if the key does not exist.
    pub fn get_preference(&self, key: &str) -> Result<Option<String>, DiskDeckError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT value FROM preferences WHERE key = ?1")?;
        let result = stmt
            .query_row(params![key], |row| row.get::<_, String>(0))
            .optional()?;
        Ok(result)
    }

    /// Sets a preference value, inserting or replacing if the key exists.
    pub fn set_preference(&self, key: &str, value: &str) -> Result<(), DiskDeckError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO preferences (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// Imports disk configs from a legacy `disks.json` file into the database.
    ///
    /// This is a one-time migration for users upgrading from the pre-database
    /// version. The import is skipped if:
    /// - The JSON file does not exist.
    /// - The database already contains disks (to prevent duplicate imports).
    ///
    /// # Returns
    ///
    /// The number of disks imported (0 if skipped).
    pub fn import_from_json(&self, json_path: &Path) -> Result<usize, DiskDeckError> {
        if !json_path.exists() {
            return Ok(0);
        }
        let existing = self.load()?;
        if !existing.is_empty() {
            return Ok(0);
        }
        let data = std::fs::read_to_string(json_path)?;
        let disks: Vec<DiskConfig> =
            serde_json::from_str(&data).map_err(|e| DiskDeckError::Database(e.to_string()))?;
        let count = disks.len();
        if count > 0 {
            self.save(&disks)?;
        }
        Ok(count)
    }

    // ─── FTS5 Index Operations ─────────────────────────────────────────

    /// Bulk-indexes a disk: clears all existing entries for the disk, then
    /// inserts the new set in a single transaction.
    ///
    /// The FTS5 triggers fire for each delete and insert, keeping the
    /// `search_index` virtual table in sync.
    ///
    /// # Performance
    ///
    /// Using a transaction is critical here — without it, each INSERT would
    /// trigger an implicit transaction and fsync, making bulk indexing of
    /// thousands of entries extremely slow.
    pub fn index_entries_bulk(
        &self,
        disk_id: &str,
        entries: &[IndexEntry],
    ) -> Result<(), DiskDeckError> {
        let conn = self.conn()?;
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM search_entries WHERE disk_id = ?1",
            params![disk_id],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO search_entries (disk_id, path, name, is_dir, size, modified) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for entry in entries {
                stmt.execute(params![
                    disk_id,
                    entry.path,
                    entry.name,
                    entry.is_dir as i32,
                    entry.size as i64,
                    entry.modified,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Inserts or updates a single entry in the search index.
    ///
    /// Checks whether an entry already exists at the given (disk_id, path) pair.
    /// If it does, updates the existing row (which fires the UPDATE trigger to
    /// refresh FTS5). If not, inserts a new row (fires the INSERT trigger).
    pub fn index_entry(
        &self,
        disk_id: &str,
        path: &str,
        name: &str,
        is_dir: bool,
        size: u64,
        modified: Option<i64>,
    ) -> Result<(), DiskDeckError> {
        let conn = self.conn()?;
        // Check if entry exists to decide INSERT vs UPDATE (triggers differ)
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM search_entries WHERE disk_id = ?1 AND path = ?2",
                params![disk_id, path],
                |row| row.get(0),
            )
            .optional_row()?;

        if let Some(id) = existing {
            conn.execute(
                "UPDATE search_entries SET name = ?1, is_dir = ?2, size = ?3, modified = ?4 WHERE id = ?5",
                params![name, is_dir as i32, size as i64, modified, id],
            )?;
        } else {
            conn.execute(
                "INSERT INTO search_entries (disk_id, path, name, is_dir, size, modified) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![disk_id, path, name, is_dir as i32, size as i64, modified],
            )?;
        }
        Ok(())
    }

    /// Removes a single entry from the search index by exact (disk_id, path) match.
    pub fn remove_entry(&self, disk_id: &str, path: &str) -> Result<(), DiskDeckError> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM search_entries WHERE disk_id = ?1 AND path = ?2",
            params![disk_id, path],
        )?;
        Ok(())
    }

    /// Removes all entries whose path starts with the given prefix.
    ///
    /// Used when deleting or renaming a directory to clean up all child entries.
    /// Uses SQL `LIKE` with a `%` wildcard suffix.
    pub fn remove_entries_under(
        &self,
        disk_id: &str,
        path_prefix: &str,
    ) -> Result<(), DiskDeckError> {
        let conn = self.conn()?;
        let pattern = format!("{}%", path_prefix);
        conn.execute(
            "DELETE FROM search_entries WHERE disk_id = ?1 AND path LIKE ?2",
            params![disk_id, pattern],
        )?;
        Ok(())
    }

    /// Searches the FTS5 index for entries matching the given pattern.
    ///
    /// # Arguments
    ///
    /// * `pattern` — User search text, transformed via [`build_fts_query`] into
    ///   an FTS5-compatible query with prefix matching.
    /// * `disk_ids` — Optional disk ID filter. If `Some`, only entries from
    ///   these disks are returned. If `None`, all disks are searched.
    /// * `limit` — Maximum number of results to return.
    ///
    /// # Returns
    ///
    /// A `Vec<IndexEntry>` sorted by FTS5 relevance rank.
    ///
    /// # Errors
    ///
    /// Returns errors if the FTS5 query syntax is invalid or the database read fails.
    pub fn search_index(
        &self,
        pattern: &str,
        disk_ids: Option<&[String]>,
        limit: usize,
    ) -> Result<Vec<IndexEntry>, DiskDeckError> {
        let fts_query = build_fts_query(pattern);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn()?;

        // Build the SQL dynamically based on whether disk_ids are provided.
        // The JOIN links the content table (search_entries) to the FTS5 virtual
        // table (search_index) via rowid.
        let (sql, has_disk_filter) = if let Some(ids) = disk_ids {
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            let placeholders: Vec<String> =
                (0..ids.len()).map(|i| format!("?{}", i + 2)).collect();
            let sql = format!(
                "SELECT se.disk_id, se.path, se.name, se.is_dir, se.size, se.modified \
                 FROM search_entries se \
                 JOIN search_index si ON si.rowid = se.id \
                 WHERE search_index MATCH ?1 AND se.disk_id IN ({}) \
                 ORDER BY si.rank \
                 LIMIT ?{}",
                placeholders.join(", "),
                ids.len() + 2
            );
            (sql, true)
        } else {
            let sql = "SELECT se.disk_id, se.path, se.name, se.is_dir, se.size, se.modified \
                        FROM search_entries se \
                        JOIN search_index si ON si.rowid = se.id \
                        WHERE search_index MATCH ?1 \
                        ORDER BY si.rank \
                        LIMIT ?2"
                .to_string();
            (sql, false)
        };

        let mut stmt = conn.prepare(&sql)?;

        // Bind parameters dynamically. When disk_ids are provided, they are
        // inserted between the FTS query (?1) and the limit (last parameter).
        let rows = if has_disk_filter {
            let ids = disk_ids.unwrap();
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            param_values.push(Box::new(fts_query.clone()));
            for id in ids {
                param_values.push(Box::new(id.clone()));
            }
            param_values.push(Box::new(limit as i64));
            let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|p| p.as_ref()).collect();
            let rows = stmt.query_map(params_ref.as_slice(), |row| {
                Ok(IndexEntry {
                    disk_id: row.get(0)?,
                    path: row.get(1)?,
                    name: row.get(2)?,
                    is_dir: row.get::<_, i32>(3)? != 0,
                    size: row.get::<_, i64>(4)? as u64,
                    modified: row.get(5)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        } else {
            let rows = stmt.query_map(params![fts_query, limit as i64], |row| {
                Ok(IndexEntry {
                    disk_id: row.get(0)?,
                    path: row.get(1)?,
                    name: row.get(2)?,
                    is_dir: row.get::<_, i32>(3)? != 0,
                    size: row.get::<_, i64>(4)? as u64,
                    modified: row.get(5)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        Ok(rows)
    }

    /// Clears all index entries for a specific disk.
    ///
    /// Called when a disk is deleted to clean up its search data.
    pub fn clear_index(&self, disk_id: &str) -> Result<(), DiskDeckError> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM search_entries WHERE disk_id = ?1",
            params![disk_id],
        )?;
        Ok(())
    }

    /// Gets index metadata for a single disk.
    ///
    /// Returns `Ok(None)` if no metadata exists (disk has never been indexed).
    pub fn get_index_meta(&self, disk_id: &str) -> Result<Option<IndexMeta>, DiskDeckError> {
        let conn = self.conn()?;
        let result = conn
            .query_row(
                "SELECT disk_id, status, entry_count, last_indexed FROM index_meta WHERE disk_id = ?1",
                params![disk_id],
                |row| {
                    Ok(IndexMeta {
                        disk_id: row.get(0)?,
                        status: row.get(1)?,
                        entry_count: row.get(2)?,
                        last_indexed: row.get(3)?,
                    })
                },
            )
            .optional_index_meta()?;
        Ok(result)
    }

    /// Updates index metadata for a disk (upsert).
    ///
    /// Sets `last_indexed` to the current UTC timestamp only when `status` is
    /// `"ready"` (meaning indexing completed successfully).
    pub fn set_index_meta(
        &self,
        disk_id: &str,
        status: &str,
        entry_count: i64,
    ) -> Result<(), DiskDeckError> {
        let conn = self.conn()?;
        let last_indexed = if status == "ready" {
            Some(chrono::Utc::now().to_rfc3339())
        } else {
            None
        };
        conn.execute(
            "INSERT OR REPLACE INTO index_meta (disk_id, status, entry_count, last_indexed) VALUES (?1, ?2, ?3, ?4)",
            params![disk_id, status, entry_count, last_indexed],
        )?;
        Ok(())
    }

    /// Returns index metadata for all disks.
    pub fn get_all_index_meta(&self) -> Result<Vec<IndexMeta>, DiskDeckError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT disk_id, status, entry_count, last_indexed FROM index_meta",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(IndexMeta {
                disk_id: row.get(0)?,
                status: row.get(1)?,
                entry_count: row.get(2)?,
                last_indexed: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    // ─── Bookmark Operations ─────────────────────────────────────────

    /// Lists all bookmarks, ordered by creation time (newest first).
    pub fn list_bookmarks(&self) -> Result<Vec<Bookmark>, DiskDeckError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, disk_id, disk_name, path, label, created_at \
             FROM bookmarks ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Bookmark {
                id: row.get(0)?,
                disk_id: row.get(1)?,
                disk_name: row.get(2)?,
                path: row.get(3)?,
                label: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Creates a new bookmark with a generated UUID and current timestamp.
    ///
    /// # Arguments
    ///
    /// * `disk_id` — The disk this bookmark points to.
    /// * `disk_name` — Display name of the disk.
    /// * `path` — Storage-relative directory path.
    /// * `label` — User-facing label for the bookmark.
    ///
    /// # Returns
    ///
    /// The newly created [`Bookmark`] with its assigned ID and timestamp.
    pub fn add_bookmark(
        &self,
        disk_id: &str,
        disk_name: &str,
        path: &str,
        label: &str,
    ) -> Result<Bookmark, DiskDeckError> {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().to_rfc3339();
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO bookmarks (id, disk_id, disk_name, path, label, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, disk_id, disk_name, path, label, created_at],
        )?;
        Ok(Bookmark {
            id,
            disk_id: disk_id.to_string(),
            disk_name: disk_name.to_string(),
            path: path.to_string(),
            label: label.to_string(),
            created_at,
        })
    }

    /// Removes a bookmark by its ID.
    ///
    /// No-op if the bookmark does not exist.
    pub fn remove_bookmark(&self, id: &str) -> Result<(), DiskDeckError> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM bookmarks WHERE id = ?1", params![id])?;
        Ok(())
    }
}

/// Extension trait to convert `rusqlite::Result<String>` into `Option<String>`,
/// treating `QueryReturnedNoRows` as `None` rather than an error.
trait OptionalRow {
    fn optional(self) -> rusqlite::Result<Option<String>>;
}

impl OptionalRow for rusqlite::Result<String> {
    fn optional(self) -> rusqlite::Result<Option<String>> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Extension trait to convert `rusqlite::Result<i64>` into `Option<i64>`,
/// treating `QueryReturnedNoRows` as `None`.
trait OptionalI64Row {
    fn optional_row(self) -> rusqlite::Result<Option<i64>>;
}

impl OptionalI64Row for rusqlite::Result<i64> {
    fn optional_row(self) -> rusqlite::Result<Option<i64>> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Extension trait to convert `rusqlite::Result<IndexMeta>` into `Option<IndexMeta>`,
/// treating `QueryReturnedNoRows` as `None`.
trait OptionalIndexMeta {
    fn optional_index_meta(self) -> rusqlite::Result<Option<IndexMeta>>;
}

impl OptionalIndexMeta for rusqlite::Result<IndexMeta> {
    fn optional_index_meta(self) -> rusqlite::Result<Option<IndexMeta>> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Transforms a user search pattern into an FTS5-compatible query string.
///
/// Each whitespace-separated token is:
/// 1. Escaped (double quotes within the token are doubled).
/// 2. Wrapped in double quotes (for exact phrase matching).
/// 3. Followed by `*` (for prefix matching).
///
/// Example: `"my doc"` becomes `"my" * "doc" *`, which matches filenames
/// starting with "my" and containing a token starting with "doc".
///
/// Returns an empty string if the input contains no non-whitespace tokens.
fn build_fts_query(pattern: &str) -> String {
    let tokens: Vec<String> = pattern
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| {
            // Escape double quotes within the token
            let escaped = t.replace('"', "\"\"");
            format!("\"{escaped}\" *")
        })
        .collect();
    tokens.join(" ")
}

/// Runs schema migrations on the database connection.
///
/// Uses `PRAGMA user_version` to track which migrations have been applied.
/// Each migration block is guarded by a version check and increments the
/// version on completion. All DDL uses `IF NOT EXISTS` for idempotency.
fn run_migrations(conn: &Connection) -> Result<(), DiskDeckError> {
    let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if version < 1 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS disks (
                id         TEXT PRIMARY KEY NOT NULL,
                name       TEXT NOT NULL,
                disk_type  TEXT NOT NULL,
                config     TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS preferences (
                key   TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            );
            PRAGMA user_version = 1;",
        )?;
    }

    if version < 2 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS search_entries (
                id       INTEGER PRIMARY KEY,
                disk_id  TEXT NOT NULL,
                path     TEXT NOT NULL,
                name     TEXT NOT NULL,
                is_dir   INTEGER NOT NULL DEFAULT 0,
                size     INTEGER NOT NULL DEFAULT 0,
                modified INTEGER
            );

            CREATE UNIQUE INDEX IF NOT EXISTS idx_search_entries_disk_path
                ON search_entries(disk_id, path);

            CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
                name,
                content='search_entries',
                content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS search_entries_ai AFTER INSERT ON search_entries BEGIN
                INSERT INTO search_index(rowid, name) VALUES (new.id, new.name);
            END;

            CREATE TRIGGER IF NOT EXISTS search_entries_ad AFTER DELETE ON search_entries BEGIN
                INSERT INTO search_index(search_index, rowid, name) VALUES ('delete', old.id, old.name);
            END;

            CREATE TRIGGER IF NOT EXISTS search_entries_au AFTER UPDATE ON search_entries BEGIN
                INSERT INTO search_index(search_index, rowid, name) VALUES ('delete', old.id, old.name);
                INSERT INTO search_index(rowid, name) VALUES (new.id, new.name);
            END;

            CREATE TABLE IF NOT EXISTS index_meta (
                disk_id      TEXT PRIMARY KEY NOT NULL,
                status       TEXT NOT NULL DEFAULT 'stale',
                entry_count  INTEGER NOT NULL DEFAULT 0,
                last_indexed TEXT
            );

            PRAGMA user_version = 2;",
        )?;
    }

    if version < 3 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS bookmarks (
                id         TEXT PRIMARY KEY NOT NULL,
                disk_id    TEXT NOT NULL,
                disk_name  TEXT NOT NULL,
                path       TEXT NOT NULL,
                label      TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            PRAGMA user_version = 3;",
        )?;
    }

    Ok(())
}

/// Backs up a corrupt/unreadable `diskdeck.db` (if it exists) to a timestamped
/// `.bak` file + explanatory `.txt` sidecar in the same directory, then removes
/// the original. If the backup step fails, the original is left in place and
/// an error is returned so the caller does not blindly delete user data.
///
/// This is the single place that implements the "never delete without a parachute"
/// safety rule for database recovery. Called from the two recovery branches in
/// [`DiskStore::new`].
///
/// The sidecar is best-effort (does not fail the backup if it cannot be written).
fn backup_and_remove_corrupt_db(db_path: &Path, reason: &str) -> Result<(), DiskDeckError> {
    if !db_path.exists() {
        // No file to back up (e.g. first-run or already-deleted). Proceed to create fresh.
        return Ok(());
    }

    let now = chrono::Local::now();
    // Use millisecond precision to avoid filename collisions on rapid successive recoveries.
    let timestamp = format!(
        "{}-{:03}",
        now.format("%Y%m%d-%H%M%S"),
        now.timestamp_subsec_millis()
    );
    let parent = db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let backup_name = format!("diskdeck.db.corrupt.{}.bak", timestamp);
    let backup_path = parent.join(&backup_name);

    // 1. Copy the raw bytes first. This must succeed before we consider deleting.
    if let Err(e) = std::fs::copy(db_path, &backup_path) {
        let full_detail = format!(
            "Could not create backup of corrupt database before recovery (reason: {}) at {}: {}. Refusing to delete the original.",
            reason,
            db_path.display(),
            e
        );
        log::error!("{}", full_detail);
        // Return a user-visible error (Storage passes the message through IPC sanitization)
        // without embedding internal paths in the user-facing string; full details are logged.
        return Err(DiskDeckError::Storage(
            "Could not create a backup of the corrupt database before recovery. The original database file has been preserved for safety. Check the application logs for details.".to_string()
        ));
    }

    // 2. Best-effort sidecar (optional but strongly recommended per requirements).
    let sidecar_name = format!("diskdeck.db.corrupt.{}.bak.txt", timestamp);
    let sidecar_path = parent.join(&sidecar_name);
    let sidecar_content = format!(
        "DiskDeck database recovery backup\n\
         \n\
         Reason: {}\n\
         Timestamp (local): {}\n\
         Original database path: {}\n\
         Backup file: {}\n\
         \n\
         This is a raw byte-for-byte copy of the diskdeck.db file that could not be\n\
         opened/decrypted (key mismatch, keychain issue, or file corruption).\n\
         \n\
         To attempt manual recovery with a different key or SQLCipher tooling:\n\
         1. Rename the .bak file back to 'diskdeck.db' in the same directory.\n\
         2. Ensure the correct 32-byte encryption key is available in the OS keychain.\n\
         3. Restart DiskDeck.\n\
         \n\
         Power users and support staff can use this file + sidecar for post-mortem\n\
         analysis or forensic recovery. The backup is never encrypted or moved.\n",
        reason, timestamp, db_path.display(), backup_path.display()
    );
    if let Err(e) = std::fs::write(&sidecar_path, sidecar_content) {
        log::warn!(
            "Backup .bak created successfully but sidecar {} could not be written: {}",
            sidecar_path.display(),
            e
        );
        // Do not fail the whole backup for sidecar — it is user-friendly, not mandatory for safety.
    }

    // 3. Only now is it safe to delete the original.
    std::fs::remove_file(db_path)?;

    log::warn!(
        "Created timestamped backup {} (and sidecar) of corrupt database before recovery. Reason: {}. Original deleted; fresh DB will be created.",
        backup_path.display(),
        reason
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_disk(id: &str, name: &str, disk_type: DiskType) -> DiskConfig {
        DiskConfig {
            id: id.to_string(),
            name: name.to_string(),
            disk_type,
            config: serde_json::json!({ "root": "/tmp/test" }),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_load_empty_returns_empty_vec() {
        let store = DiskStore::new_in_memory().unwrap();
        let disks = store.load().unwrap();
        assert!(disks.is_empty());
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let store = DiskStore::new_in_memory().unwrap();

        let disks = vec![
            make_disk("id-1", "Local Home", DiskType::Local),
            make_disk("id-2", "S3 Bucket", DiskType::S3),
        ];
        store.save(&disks).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "id-1");
        assert_eq!(loaded[0].name, "Local Home");
        assert_eq!(loaded[0].disk_type, DiskType::Local);
        assert_eq!(loaded[1].id, "id-2");
        assert_eq!(loaded[1].name, "S3 Bucket");
        assert_eq!(loaded[1].disk_type, DiskType::S3);
    }

    #[test]
    fn test_save_overwrites_previous() {
        let store = DiskStore::new_in_memory().unwrap();

        let disks_v1 = vec![
            make_disk("id-1", "Disk A", DiskType::Local),
            make_disk("id-2", "Disk B", DiskType::Local),
        ];
        store.save(&disks_v1).unwrap();

        let disks_v2 = vec![make_disk("id-1", "Disk A", DiskType::Local)];
        store.save(&disks_v2).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "id-1");
    }

    #[test]
    fn test_save_empty_clears_all_disks() {
        let store = DiskStore::new_in_memory().unwrap();

        store
            .save(&[make_disk("id-1", "X", DiskType::Local)])
            .unwrap();
        store.save(&[]).unwrap();

        let loaded = store.load().unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_save_preserves_all_fields() {
        let store = DiskStore::new_in_memory().unwrap();

        let disk = DiskConfig {
            id: "s3-disk".to_string(),
            name: "My Bucket".to_string(),
            disk_type: DiskType::S3,
            config: serde_json::json!({
                "bucket": "my-bucket",
                "region": "us-east-1"
            }),
            created_at: "2026-03-15T12:00:00Z".to_string(),
        };
        store.save(&[disk]).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded[0].config["bucket"], "my-bucket");
        assert_eq!(loaded[0].config["region"], "us-east-1");
        assert_eq!(loaded[0].created_at, "2026-03-15T12:00:00Z");
    }

    #[test]
    fn test_update_in_place() {
        let store = DiskStore::new_in_memory().unwrap();

        let mut disks = vec![
            make_disk("id-1", "Original", DiskType::Local),
            make_disk("id-2", "Other", DiskType::Local),
        ];
        store.save(&disks).unwrap();

        disks[0].name = "Renamed".to_string();
        disks[0].config = serde_json::json!({ "root": "/new/path" });
        store.save(&disks).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "Renamed");
        assert_eq!(loaded[0].config["root"], "/new/path");
        assert_eq!(loaded[1].name, "Other");
    }

    #[test]
    fn test_delete_disk_by_filtering() {
        let store = DiskStore::new_in_memory().unwrap();

        let disks = vec![
            make_disk("id-1", "Keep", DiskType::Local),
            make_disk("id-2", "Remove", DiskType::S3),
        ];
        store.save(&disks).unwrap();

        let remaining: Vec<_> = disks.into_iter().filter(|d| d.id != "id-2").collect();
        store.save(&remaining).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "id-1");
        assert_eq!(loaded[0].name, "Keep");
    }

    #[test]
    fn test_creates_data_dir_if_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        let nested = dir.path().join("deeply").join("nested").join("dir");
        assert!(!nested.exists());

        // Use file-backed DB to test directory creation
        let store = DiskStore::new(&nested, "").unwrap();
        assert!(nested.exists());

        store
            .save(&[make_disk("id-1", "Test", DiskType::Local)])
            .unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn test_corrupt_db_gets_replaced() {
        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("diskdeck.db");

        // Write garbage to simulate an encrypted/corrupt database file
        std::fs::write(&db_path, b"not a sqlite database at all").unwrap();

        // DiskStore::new should detect the corruption, *backup* the file first
        // (creating .bak + .txt sidecar), delete the original, and create fresh DB.
        let store = DiskStore::new(dir.path(), "").unwrap();
        let disks = store.load().unwrap();
        assert!(disks.is_empty());

        // Verify the fresh database is functional
        store
            .save(&[make_disk("id-1", "Test", DiskType::Local)])
            .unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);

        // Critical safety check: a timestamped backup + sidecar must exist next to the (now-deleted) original
        let dir_entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        let bak_file = dir_entries.iter().find(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.contains("diskdeck.db.corrupt.") && name.ends_with(".bak")
        });
        assert!(
            bak_file.is_some(),
            "expected a timestamped .bak backup file to have been created before deleting the corrupt DB"
        );

        let txt_file = dir_entries.iter().find(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.contains("diskdeck.db.corrupt.") && name.ends_with(".bak.txt")
        });
        assert!(
            txt_file.is_some(),
            "expected a .bak.txt sidecar explaining the recovery to have been created"
        );
    }

    #[test]
    fn test_get_preference_missing_returns_none() {
        let store = DiskStore::new_in_memory().unwrap();
        let val = store.get_preference("nonexistent").unwrap();
        assert!(val.is_none());
    }

    #[test]
    fn test_set_and_get_preference_roundtrip() {
        let store = DiskStore::new_in_memory().unwrap();
        store.set_preference("theme", "dark").unwrap();
        let val = store.get_preference("theme").unwrap();
        assert_eq!(val, Some("dark".to_string()));
    }

    #[test]
    fn test_set_preference_overwrites() {
        let store = DiskStore::new_in_memory().unwrap();
        store.set_preference("theme", "dark").unwrap();
        store.set_preference("theme", "light").unwrap();
        let val = store.get_preference("theme").unwrap();
        assert_eq!(val, Some("light".to_string()));
    }

    #[test]
    fn test_migration_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        // Open twice — migrations should not fail on second open
        {
            let _store = DiskStore::new(dir.path(), "").unwrap();
        }
        let store = DiskStore::new(dir.path(), "").unwrap();
        let disks = store.load().unwrap();
        assert!(disks.is_empty());
    }

    #[test]
    fn test_save_multiple_disks() {
        let store = DiskStore::new_in_memory().unwrap();

        let disks = vec![
            make_disk("id-1", "Local", DiskType::Local),
            make_disk("id-2", "S3", DiskType::S3),
            make_disk("id-3", "Azure", DiskType::Azure),
        ];
        store.save(&disks).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].disk_type, DiskType::Local);
        assert_eq!(loaded[1].disk_type, DiskType::S3);
        assert_eq!(loaded[2].disk_type, DiskType::Azure);
    }

    #[test]
    fn test_load_after_save_returns_latest() {
        let store = DiskStore::new_in_memory().unwrap();

        store
            .save(&[make_disk("id-1", "V1", DiskType::Local)])
            .unwrap();
        store
            .save(&[make_disk("id-1", "V2", DiskType::Local)])
            .unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "V2");
    }

    #[test]
    fn test_import_from_json_empty_file() {
        let store = DiskStore::new_in_memory().unwrap();
        let dir = tempfile::TempDir::new().unwrap();
        let json_path = dir.path().join("disks.json");

        // Non-existent file — no import
        let count = store.import_from_json(&json_path).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_import_from_json_with_data() {
        let store = DiskStore::new_in_memory().unwrap();
        let dir = tempfile::TempDir::new().unwrap();
        let json_path = dir.path().join("disks.json");

        let disks = vec![make_disk("id-1", "Imported", DiskType::Local)];
        let json = serde_json::to_string(&disks).unwrap();
        std::fs::write(&json_path, json).unwrap();

        let count = store.import_from_json(&json_path).unwrap();
        assert_eq!(count, 1);

        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Imported");
    }

    #[test]
    fn test_import_from_json_skips_if_db_has_data() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .save(&[make_disk("existing", "Existing", DiskType::Local)])
            .unwrap();

        let dir = tempfile::TempDir::new().unwrap();
        let json_path = dir.path().join("disks.json");
        let disks = vec![make_disk("id-1", "FromJson", DiskType::Local)];
        std::fs::write(&json_path, serde_json::to_string(&disks).unwrap()).unwrap();

        let count = store.import_from_json(&json_path).unwrap();
        assert_eq!(count, 0); // Skipped because DB already has data

        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Existing");
    }

    // ─── FTS5 Index Tests ───

    fn make_index_entry(path: &str, name: &str) -> IndexEntry {
        IndexEntry {
            disk_id: String::new(), // set by bulk method
            path: path.to_string(),
            name: name.to_string(),
            is_dir: false,
            size: 1024,
            modified: Some(1700000000),
        }
    }

    #[test]
    fn test_migration_v2_creates_fts_tables() {
        let store = DiskStore::new_in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        // Verify search_entries table exists
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM search_entries",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        // Verify index_meta table exists
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM index_meta", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        // Verify FTS5 virtual table exists
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM search_index", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_index_entry_and_search() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .index_entry("disk-1", "/readme.md", "readme.md", false, 100, Some(1700000000))
            .unwrap();
        store
            .index_entry("disk-1", "/docs/guide.txt", "guide.txt", false, 200, None)
            .unwrap();

        let results = store.search_index("readme", None, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "/readme.md");
        assert_eq!(results[0].name, "readme.md");
        assert_eq!(results[0].size, 100);
    }

    #[test]
    fn test_search_prefix_match() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .index_entry("disk-1", "/document.pdf", "document.pdf", false, 500, None)
            .unwrap();
        store
            .index_entry("disk-1", "/data.csv", "data.csv", false, 300, None)
            .unwrap();

        // "doc" should match "document.pdf" via prefix
        let results = store.search_index("doc", None, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "document.pdf");
    }

    #[test]
    fn test_remove_entry() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .index_entry("disk-1", "/file.txt", "file.txt", false, 100, None)
            .unwrap();
        store.remove_entry("disk-1", "/file.txt").unwrap();

        let results = store.search_index("file", None, 100).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_remove_entries_under() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .index_entry("disk-1", "/photos/a.jpg", "a.jpg", false, 100, None)
            .unwrap();
        store
            .index_entry("disk-1", "/photos/sub/b.png", "b.png", false, 200, None)
            .unwrap();
        store
            .index_entry("disk-1", "/docs/c.txt", "c.txt", false, 50, None)
            .unwrap();

        store.remove_entries_under("disk-1", "/photos").unwrap();

        // Photos entries gone, docs entry remains
        let results = store.search_index("a", None, 100).unwrap();
        assert!(results.is_empty());
        let results = store.search_index("b", None, 100).unwrap();
        assert!(results.is_empty());
        let results = store.search_index("c", None, 100).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_bulk_index_replaces_existing() {
        let store = DiskStore::new_in_memory().unwrap();

        let entries_v1 = vec![
            make_index_entry("/old.txt", "old.txt"),
        ];
        store.index_entries_bulk("disk-1", &entries_v1).unwrap();

        let entries_v2 = vec![
            make_index_entry("/new.txt", "new.txt"),
        ];
        store.index_entries_bulk("disk-1", &entries_v2).unwrap();

        // Old entry should be gone
        let results = store.search_index("old", None, 100).unwrap();
        assert!(results.is_empty());
        // New entry should be present
        let results = store.search_index("new", None, 100).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_clear_index() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .index_entry("disk-1", "/file.txt", "file.txt", false, 100, None)
            .unwrap();
        store.clear_index("disk-1").unwrap();

        let results = store.search_index("file", None, 100).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_scoped_to_disk() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .index_entry("disk-a", "/report.pdf", "report.pdf", false, 100, None)
            .unwrap();
        store
            .index_entry("disk-b", "/report.docx", "report.docx", false, 200, None)
            .unwrap();

        // Search scoped to disk-a
        let results = store
            .search_index("report", Some(&["disk-a".to_string()]), 100)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].disk_id, "disk-a");

        // Search all disks
        let results = store.search_index("report", None, 100).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_index_meta_roundtrip() {
        let store = DiskStore::new_in_memory().unwrap();

        // Initially no meta
        let meta = store.get_index_meta("disk-1").unwrap();
        assert!(meta.is_none());

        // Set to indexing
        store.set_index_meta("disk-1", "indexing", 0).unwrap();
        let meta = store.get_index_meta("disk-1").unwrap().unwrap();
        assert_eq!(meta.status, "indexing");
        assert_eq!(meta.entry_count, 0);
        assert!(meta.last_indexed.is_none());

        // Set to ready (should set last_indexed)
        store.set_index_meta("disk-1", "ready", 42).unwrap();
        let meta = store.get_index_meta("disk-1").unwrap().unwrap();
        assert_eq!(meta.status, "ready");
        assert_eq!(meta.entry_count, 42);
        assert!(meta.last_indexed.is_some());

        // get_all_index_meta
        store.set_index_meta("disk-2", "stale", 0).unwrap();
        let all = store.get_all_index_meta().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_fts_special_characters() {
        let store = DiskStore::new_in_memory().unwrap();
        store
            .index_entry("disk-1", "/my-file.test.txt", "my-file.test.txt", false, 100, None)
            .unwrap();
        store
            .index_entry("disk-1", "/hello world.pdf", "hello world.pdf", false, 200, None)
            .unwrap();
        store
            .index_entry("disk-1", "/under_score.rs", "under_score.rs", false, 50, None)
            .unwrap();

        // Search with dot
        let results = store.search_index("my-file", None, 100).unwrap();
        assert!(!results.is_empty());

        // Search with space
        let results = store.search_index("hello", None, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "hello world.pdf");

        // Search with underscore
        let results = store.search_index("under_score", None, 100).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_migration_v2_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        {
            let _store = DiskStore::new(dir.path(), "").unwrap();
        }
        // Open again — migration v2 should not fail
        let store = DiskStore::new(dir.path(), "").unwrap();
        store
            .index_entry("disk-1", "/test.txt", "test.txt", false, 100, None)
            .unwrap();
        let results = store.search_index("test", None, 100).unwrap();
        assert_eq!(results.len(), 1);
    }

    // ─── Bookmark Tests ───

    #[test]
    fn test_bookmark_roundtrip() {
        let store = DiskStore::new_in_memory().unwrap();
        let bookmark = store
            .add_bookmark("disk-1", "My Disk", "/docs", "Documents")
            .unwrap();

        assert_eq!(bookmark.disk_id, "disk-1");
        assert_eq!(bookmark.disk_name, "My Disk");
        assert_eq!(bookmark.path, "/docs");
        assert_eq!(bookmark.label, "Documents");
        assert!(!bookmark.id.is_empty());
        assert!(!bookmark.created_at.is_empty());

        let bookmarks = store.list_bookmarks().unwrap();
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].id, bookmark.id);
        assert_eq!(bookmarks[0].label, "Documents");
    }

    #[test]
    fn test_remove_bookmark() {
        let store = DiskStore::new_in_memory().unwrap();
        let bookmark = store
            .add_bookmark("disk-1", "My Disk", "/docs", "Documents")
            .unwrap();

        store.remove_bookmark(&bookmark.id).unwrap();

        let bookmarks = store.list_bookmarks().unwrap();
        assert!(bookmarks.is_empty());
    }

    #[test]
    fn test_bookmark_migration_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        {
            let store = DiskStore::new(dir.path(), "").unwrap();
            store
                .add_bookmark("disk-1", "D", "/path", "Label")
                .unwrap();
        }
        // Open again — migration v3 should not fail
        let store = DiskStore::new(dir.path(), "").unwrap();
        let bookmarks = store.list_bookmarks().unwrap();
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].label, "Label");
    }
}
