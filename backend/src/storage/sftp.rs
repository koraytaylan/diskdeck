//! # SFTP storage backend
//!
//! Implements [`StorageBackend`] over SSH File Transfer Protocol using the
//! `russh` (SSH) and `russh-sftp` (SFTP subsystem) crates.
//!
//! ## SSH connection lifecycle
//!
//! The backend maintains a single SFTP session behind a `tokio::sync::Mutex`.
//! Connections are established lazily on first use via [`SftpBackend::get_sftp`].
//!
//! ## Reconnect strategy
//!
//! If an SFTP operation fails, the backend:
//! 1. Drops the mutex guard (releasing the lock).
//! 2. Calls [`SftpBackend::reset_conn`] to set the connection to `None`.
//! 3. Returns the error to the caller.
//!
//! The **next** operation that acquires the mutex will see `None` and trigger a
//! fresh `connect()`. This gives a simple form of automatic reconnection without
//! retry loops.
//!
//! ## Security: host key verification
//!
//! The current [`SshHandler`] accepts **all** server host keys (`check_server_key`
//! returns `true`). This is a known limitation suitable for trusted/internal
//! networks. A production hardening step would be to implement known-hosts
//! checking or first-use trust-on-first-use (TOFU).
//!
//! ## Path normalization
//!
//! SFTP paths are relative to the server's root. DiskDeck virtual paths start
//! with `/`; the backend maps:
//! - `"/"` or `""` -> `"."` (SFTP current directory, typically the user's home).
//! - `"/foo/bar"` -> `"foo/bar"` (stripped leading slash).
//!
//! ## Limitations
//!
//! - **No server-side copy**: SFTP has no copy command. `copy()` downloads the
//!   file and re-uploads it to the destination.
//! - **Two auth modes**: Password authentication and SSH key-based authentication.
//!   When `key_path` is set, public key auth is used; otherwise, password auth.
//! - **Single connection**: All operations serialize through one SFTP session.
//!   Concurrent file operations will queue behind the mutex.

use std::sync::Arc;

use async_trait::async_trait;
use russh::client;
use russh::keys::key;
use russh_sftp::client::SftpSession;
use tokio::sync::Mutex;

use crate::error::DiskDeckError;
use crate::models::entry::Entry;
use crate::models::search::SearchQuery;

use super::{StorageBackend, StorageResult};

/// Minimal SSH client handler that accepts all host keys.
///
/// See module-level docs for the security implications.
struct SshHandler;

#[async_trait]
impl client::Handler for SshHandler {
    type Error = russh::Error;

    /// Accepts any server public key unconditionally.
    async fn check_server_key(
        &mut self,
        _server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Internal holder for a live SSH + SFTP session pair.
///
/// The `_ssh` handle must be kept alive for the duration of the SFTP session;
/// dropping it would close the underlying SSH transport.
struct SftpConnection {
    /// The SSH session handle. Kept alive but not directly used after setup.
    _ssh: client::Handle<SshHandler>,
    /// The SFTP subsystem session used for all file operations.
    sftp: SftpSession,
}

/// SFTP storage backend via `russh` + `russh-sftp`.
///
/// Connects to a remote server over SSH and operates on files via the SFTP
/// subsystem. Connection credentials are stored in the struct for reconnection.
///
/// Supports two authentication modes:
/// - **Password**: Traditional username/password (when `key_path` is `None`).
/// - **SSH key**: Public key authentication from a private key file on disk
///   (when `key_path` is `Some`). The password field is ignored in this mode.
pub struct SftpBackend {
    host: String,
    port: u16,
    username: String,
    password: String,
    /// Optional path to an SSH private key file for public key authentication.
    /// When `Some`, password authentication is skipped in favor of key-based auth.
    key_path: Option<String>,
    /// Lazily-initialized SFTP connection, protected by a mutex for
    /// serialized access and safe reconnection.
    conn: Mutex<Option<SftpConnection>>,
}

/// Converts a `SystemTime` to a Unix epoch timestamp (seconds).
/// Returns 0 if the time is before the epoch.
fn system_time_to_unix(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl SftpBackend {
    /// Creates a new SftpBackend with the given connection parameters.
    ///
    /// No connection is established at this point; it will be created lazily
    /// on the first operation.
    ///
    /// # Arguments
    ///
    /// * `host` - Hostname or IP address of the SSH server.
    /// * `port` - SSH port (typically 22).
    /// * `username` - SSH username.
    /// * `password` - SSH password (ignored when `key_path` is provided).
    /// * `key_path` - Optional path to an SSH private key file. When provided,
    ///   public key authentication is used instead of password authentication.
    pub fn new(host: &str, port: u16, username: &str, password: &str, key_path: Option<&str>) -> Self {
        Self {
            host: host.to_string(),
            port,
            username: username.to_string(),
            password: password.to_string(),
            key_path: key_path.map(|s| s.to_string()),
            conn: Mutex::new(None),
        }
    }

    /// Establishes a fresh SSH connection and opens an SFTP subsystem.
    ///
    /// The connection flow is:
    /// 1. TCP connect to `host:port`.
    /// 2. Authenticate with SSH key (if `key_path` is set) or password.
    /// 3. Open an SSH channel session.
    /// 4. Request the "sftp" subsystem on that channel.
    /// 5. Wrap the channel stream in an `SftpSession`.
    async fn connect(&self) -> StorageResult<SftpConnection> {
        let config = Arc::new(client::Config::default());
        let handler = SshHandler;

        let mut session =
            client::connect(config, (&*self.host, self.port), handler)
                .await
                .map_err(|e| DiskDeckError::Storage(format!("SSH connect: {e}")))?;

        if let Some(ref kp) = self.key_path {
            let key_pair = russh_keys::load_secret_key(kp, None)
                .map_err(|e| DiskDeckError::Storage(format!("Failed to load SSH key: {e}")))?;
            let auth = session
                .authenticate_publickey(&self.username, Arc::new(key_pair))
                .await
                .map_err(|e| DiskDeckError::Storage(format!("SSH key auth: {e}")))?;
            if !auth {
                return Err(DiskDeckError::Storage("SSH key authentication failed".into()));
            }
        } else {
            let auth = session
                .authenticate_password(&self.username, &self.password)
                .await
                .map_err(|e| DiskDeckError::Storage(format!("SSH auth: {e}")))?;
            if !auth {
                return Err(DiskDeckError::Storage("SSH authentication failed".into()));
            }
        }

        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| DiskDeckError::Storage(format!("SSH channel: {e}")))?;

        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| DiskDeckError::Storage(format!("SFTP subsystem: {e}")))?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| DiskDeckError::Storage(format!("SFTP session: {e}")))?;

        Ok(SftpConnection {
            _ssh: session,
            sftp,
        })
    }

    /// Returns a mutex guard to the SFTP session, connecting if necessary.
    ///
    /// If the connection was previously reset (due to an error), this will
    /// establish a new one transparently.
    async fn get_sftp(&self) -> StorageResult<tokio::sync::MutexGuard<'_, Option<SftpConnection>>> {
        let mut guard = self.conn.lock().await;
        if guard.is_none() {
            *guard = Some(self.connect().await?);
        }
        Ok(guard)
    }

    /// Drops the current connection so the next operation triggers a reconnect.
    ///
    /// Called after any SFTP operation fails, implementing the lazy-reconnect
    /// strategy described in the module docs.
    async fn reset_conn(&self) {
        let mut guard = self.conn.lock().await;
        *guard = None;
    }

    /// Converts a DiskDeck virtual path to an SFTP remote path.
    ///
    /// - `"/"` or `""` -> `"."` (SFTP current working directory).
    /// - `"/foo/bar"` -> `"foo/bar"`.
    fn normalize_path(path: &str) -> String {
        if path.is_empty() || path == "/" {
            ".".to_string()
        } else {
            path.strip_prefix('/').unwrap_or(path).to_string()
        }
    }

    /// Builds a DiskDeck virtual entry path from an SFTP directory path and filename.
    fn to_entry_path(remote_path: &str, name: &str) -> String {
        if remote_path == "." || remote_path.is_empty() {
            format!("/{}", name)
        } else {
            let base = remote_path.strip_prefix('/').unwrap_or(remote_path);
            format!("/{}/{}", base, name)
        }
    }
}

#[async_trait]
impl StorageBackend for SftpBackend {
    async fn list(&self, path: &str) -> StorageResult<Vec<Entry>> {
        let remote_path = Self::normalize_path(path);
        let guard = self.get_sftp().await?;
        let sftp = &guard.as_ref().unwrap().sftp;

        let dir_entries = match sftp.read_dir(&remote_path).await {
            Ok(entries) => entries,
            Err(e) => {
                drop(guard);
                self.reset_conn().await;
                return Err(DiskDeckError::Storage(format!("SFTP readdir: {e}")));
            }
        };

        let mut entries = Vec::new();
        for de in dir_entries {
            let name = de.file_name();
            // Skip Unix special directory entries
            if name == "." || name == ".." {
                continue;
            }
            let is_dir = de.file_type().is_dir();
            let size = de.metadata().len();
            let modified = de
                .metadata()
                .modified()
                .ok()
                .map(system_time_to_unix);
            let mime = if !is_dir {
                mime_guess::from_path(&name)
                    .first()
                    .map(|m| m.to_string())
            } else {
                None
            };

            entries.push(Entry {
                path: Self::to_entry_path(&remote_path, &name),
                name,
                size,
                modified,
                created: None,
                is_dir,
                permissions: None,
                mime_type: mime,
            });
        }

        // Sort: directories first, then alphabetical (case-insensitive)
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(entries)
    }

    async fn read(&self, path: &str) -> StorageResult<Vec<u8>> {
        let remote_path = Self::normalize_path(path);
        let guard = self.get_sftp().await?;
        let sftp = &guard.as_ref().unwrap().sftp;

        match sftp.read(&remote_path).await {
            Ok(data) => Ok(data),
            Err(e) => {
                drop(guard);
                self.reset_conn().await;
                Err(DiskDeckError::Storage(format!("SFTP read: {e}")))
            }
        }
    }

    async fn write(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        let remote_path = Self::normalize_path(path);
        let guard = self.get_sftp().await?;
        let sftp = &guard.as_ref().unwrap().sftp;

        match sftp.write(&remote_path, data).await {
            Ok(()) => Ok(()),
            Err(e) => {
                drop(guard);
                self.reset_conn().await;
                Err(DiskDeckError::Storage(format!("SFTP write: {e}")))
            }
        }
    }

    /// Deletes a file or directory. Tries file removal first; if that fails
    /// (e.g., it is a directory), falls back to `remove_dir`.
    ///
    /// Note: `remove_dir` only works on empty directories. Recursive deletion
    /// would require walking the tree, which is not implemented here (the
    /// frontend handles multi-path deletion at the command level).
    async fn delete(&self, path: &str) -> StorageResult<()> {
        let remote_path = Self::normalize_path(path);
        let guard = self.get_sftp().await?;
        let sftp = &guard.as_ref().unwrap().sftp;

        match sftp.remove_file(&remote_path).await {
            Ok(()) => Ok(()),
            Err(_) => match sftp.remove_dir(&remote_path).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    drop(guard);
                    self.reset_conn().await;
                    Err(DiskDeckError::Storage(format!("SFTP delete: {e}")))
                }
            },
        }
    }

    /// Copies a file by downloading it and re-uploading to the destination.
    /// SFTP has no server-side copy command.
    async fn copy(&self, src: &str, dst: &str) -> StorageResult<()> {
        let data = self.read(src).await?;
        self.write(dst, &data).await
    }

    async fn rename(&self, src: &str, dst: &str) -> StorageResult<()> {
        let src_path = Self::normalize_path(src);
        let dst_path = Self::normalize_path(dst);
        let guard = self.get_sftp().await?;
        let sftp = &guard.as_ref().unwrap().sftp;

        match sftp.rename(&src_path, &dst_path).await {
            Ok(()) => Ok(()),
            Err(e) => {
                drop(guard);
                self.reset_conn().await;
                Err(DiskDeckError::Storage(format!("SFTP rename: {e}")))
            }
        }
    }

    async fn stat(&self, path: &str) -> StorageResult<Entry> {
        let remote_path = Self::normalize_path(path);
        let guard = self.get_sftp().await?;
        let sftp = &guard.as_ref().unwrap().sftp;

        let attrs = match sftp.metadata(&remote_path).await {
            Ok(a) => a,
            Err(e) => {
                drop(guard);
                self.reset_conn().await;
                return Err(DiskDeckError::Storage(format!("SFTP stat: {e}")));
            }
        };

        let name = remote_path
            .rsplit('/')
            .next()
            .unwrap_or(&remote_path)
            .to_string();

        let is_dir = attrs.is_dir();
        let size = attrs.len();
        let modified = attrs.modified().ok().map(system_time_to_unix);

        let path_str = if remote_path == "." {
            "/".to_string()
        } else {
            format!("/{}", remote_path)
        };

        let mime = if !is_dir {
            mime_guess::from_path(&name)
                .first()
                .map(|m| m.to_string())
        } else {
            None
        };

        Ok(Entry {
            path: path_str,
            name,
            size,
            modified,
            created: None,
            is_dir,
            permissions: None,
            mime_type: mime,
        })
    }

    async fn exists(&self, path: &str) -> StorageResult<bool> {
        match self.stat(path).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Searches by walking the remote directory tree via repeated `read_dir` calls.
    ///
    /// This uses a depth-first iterative approach (stack of directories to visit).
    /// Each directory listing requires re-acquiring the SFTP mutex, which means
    /// the lock is released between directories — allowing other operations to
    /// interleave. Errors reading a directory are silently skipped.
    async fn search(&self, query: &SearchQuery) -> StorageResult<Vec<Entry>> {
        let pattern = query.pattern.to_lowercase();
        let recursive = query.recursive;

        let mut results = Vec::new();
        let mut dirs_to_visit = vec![".".to_string()];

        while let Some(dir) = dirs_to_visit.pop() {
            let guard = self.get_sftp().await?;
            let sftp = &guard.as_ref().unwrap().sftp;

            let dir_entries = match sftp.read_dir(&dir).await {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            for de in dir_entries {
                let name = de.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                let is_dir = de.file_type().is_dir();
                let size = de.metadata().len();
                let modified = de.metadata().modified().ok().map(system_time_to_unix);
                let entry_path = Self::to_entry_path(&dir, &name);
                let mime = if !is_dir {
                    mime_guess::from_path(&name)
                        .first()
                        .map(|m| m.to_string())
                } else {
                    None
                };

                let entry = Entry {
                    path: entry_path,
                    name: name.clone(),
                    size,
                    modified,
                    created: None,
                    is_dir,
                    permissions: None,
                    mime_type: mime,
                };

                if name.to_lowercase().contains(&pattern) {
                    results.push(entry.clone());
                }
                if recursive && is_dir {
                    let sub_path = entry.path.strip_prefix('/').unwrap_or(&entry.path);
                    dirs_to_visit.push(sub_path.to_string());
                }
            }
        }

        Ok(results)
    }

    async fn create_dir(&self, path: &str) -> StorageResult<()> {
        let remote_path = Self::normalize_path(path);
        let guard = self.get_sftp().await?;
        let sftp = &guard.as_ref().unwrap().sftp;

        match sftp.create_dir(&remote_path).await {
            Ok(()) => Ok(()),
            Err(e) => {
                drop(guard);
                self.reset_conn().await;
                Err(DiskDeckError::Storage(format!("SFTP mkdir: {e}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_root() {
        assert_eq!(SftpBackend::normalize_path("/"), ".");
    }

    #[test]
    fn normalize_path_empty() {
        assert_eq!(SftpBackend::normalize_path(""), ".");
    }

    #[test]
    fn normalize_path_strips_leading_slash() {
        assert_eq!(SftpBackend::normalize_path("/foo/bar"), "foo/bar");
    }

    #[test]
    fn normalize_path_no_slash() {
        assert_eq!(SftpBackend::normalize_path("foo/bar"), "foo/bar");
    }

    #[test]
    fn to_entry_path_root() {
        assert_eq!(SftpBackend::to_entry_path(".", "file.txt"), "/file.txt");
    }

    #[test]
    fn to_entry_path_empty() {
        assert_eq!(SftpBackend::to_entry_path("", "file.txt"), "/file.txt");
    }

    #[test]
    fn to_entry_path_nested() {
        assert_eq!(SftpBackend::to_entry_path("home/user", "docs"), "/home/user/docs");
    }

    #[test]
    fn to_entry_path_with_leading_slash() {
        assert_eq!(SftpBackend::to_entry_path("/home/user", "docs"), "/home/user/docs");
    }

    #[test]
    fn system_time_to_unix_epoch() {
        let t = std::time::UNIX_EPOCH;
        assert_eq!(system_time_to_unix(t), 0);
    }

    #[test]
    fn system_time_to_unix_future() {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000);
        assert_eq!(system_time_to_unix(t), 1000);
    }

    #[test]
    fn new_creates_backend_without_key() {
        let backend = SftpBackend::new("example.com", 22, "user", "pass", None);
        assert_eq!(backend.host, "example.com");
        assert_eq!(backend.port, 22);
        assert_eq!(backend.username, "user");
        assert_eq!(backend.password, "pass");
        assert!(backend.key_path.is_none());
    }

    #[test]
    fn new_creates_backend_with_key_path() {
        let backend = SftpBackend::new("example.com", 22, "user", "", Some("/home/user/.ssh/id_rsa"));
        assert_eq!(backend.host, "example.com");
        assert_eq!(backend.port, 22);
        assert_eq!(backend.username, "user");
        assert_eq!(backend.key_path, Some("/home/user/.ssh/id_rsa".to_string()));
    }
}
