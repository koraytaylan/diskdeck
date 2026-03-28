//! # FTP/FTPS storage backend
//!
//! Implements [`StorageBackend`] for FTP servers using the `suppaftp` crate
//! with native-TLS support.
//!
//! ## FTP protocol specifics
//!
//! FTP is a stateful, connection-oriented protocol where the server maintains
//! a "current directory" and transfers happen over separate data connections.
//! Key differences from HTTP-based backends (S3, Azure):
//!
//! - **Connection reuse**: A single control connection is kept open and reused
//!   across operations (behind a `tokio::sync::Mutex`, same pattern as SFTP).
//! - **Binary mode**: Set immediately after login to prevent newline translation
//!   that would corrupt binary files.
//! - **Reconnect on failure**: Same lazy-reconnect strategy as SFTP — on error,
//!   the connection is reset and re-established on the next operation.
//!
//! ## TLS upgrade
//!
//! If `tls` is `true` in the config, the backend first connects in plaintext
//! and then upgrades to TLS via `AUTH TLS` (explicit FTPS). This is the most
//! widely supported FTPS mode. Implicit FTPS (port 990) is not supported.
//!
//! ## LIST parsing
//!
//! FTP's `LIST` command returns a human-readable directory listing in a
//! format that is not formally standardized. The [`parse_list_line`] function
//! parses the common **Unix-style** format:
//!
//! ```text
//! drwxr-xr-x  2 user group 4096 Jan  1 12:00 dirname
//! -rw-r--r--  1 user group 1234 Jan  1 12:00 filename.txt
//! ```
//!
//! Parsing rules:
//! - First character `d` indicates a directory.
//! - Field 5 (0-indexed: 4) is the file size.
//! - Everything after the 8th whitespace-separated field is the filename
//!   (this handles filenames with spaces).
//! - Lines with fewer than 9 fields, or entries named `.` or `..`, are skipped.
//!
//! **Gotcha**: Some FTP servers use Windows-style LIST output, which this parser
//! does not handle. A future enhancement could add MLSD support.
//!
//! ## Limitations
//!
//! - **No server-side copy**: FTP has no copy command. `copy()` downloads and
//!   re-uploads.
//! - **stat() via LIST**: FTP has no `STAT` for file metadata. `stat()` lists
//!   the parent directory and finds the entry by name, which is an extra
//!   round-trip.
//! - **No modified timestamps**: The LIST parser does not extract modification
//!   times (would require parsing locale-dependent date strings).
//! - **Single connection**: Same serialization constraint as SFTP.

use std::future::Future;
use std::pin::Pin;

use async_trait::async_trait;
use futures::io::AsyncReadExt;
use suppaftp::AsyncNativeTlsFtpStream;
use tokio::sync::Mutex;

use crate::error::DiskDeckError;
use crate::models::entry::Entry;
use crate::models::search::SearchQuery;

use super::{StorageBackend, StorageResult};

/// FTP/FTPS storage backend via `suppaftp`.
///
/// Holds connection credentials and a lazily-initialized FTP stream behind
/// a mutex. The `tls` flag controls whether explicit FTPS is used.
pub struct FtpBackend {
    host: String,
    port: u16,
    username: String,
    password: String,
    /// Whether to upgrade the connection to TLS via AUTH TLS (explicit FTPS).
    tls: bool,
    /// Lazily-initialized FTP connection, protected by a mutex.
    conn: Mutex<Option<AsyncNativeTlsFtpStream>>,
}

impl FtpBackend {
    /// Creates a new FtpBackend with the given connection parameters.
    ///
    /// No connection is established until the first operation.
    ///
    /// # Arguments
    ///
    /// * `host` — FTP server hostname or IP address.
    /// * `port` — FTP server port (typically 21).
    /// * `username` — FTP login username.
    /// * `password` — FTP login password.
    /// * `tls` — If `true`, upgrade to explicit FTPS after connecting.
    pub fn new(host: &str, port: u16, username: &str, password: &str, tls: bool) -> Self {
        Self {
            host: host.to_string(),
            port,
            username: username.to_string(),
            password: password.to_string(),
            tls,
            conn: Mutex::new(None),
        }
    }

    /// Establishes a fresh FTP connection, optionally upgrading to TLS.
    ///
    /// The connection flow:
    /// 1. TCP connect to `host:port`.
    /// 2. If TLS is enabled, negotiate explicit FTPS via `AUTH TLS`.
    /// 3. Authenticate with username/password.
    /// 4. Switch to binary transfer mode to prevent newline corruption.
    async fn connect(&self) -> StorageResult<AsyncNativeTlsFtpStream> {
        let addr = format!("{}:{}", self.host, self.port);

        let mut ftp = AsyncNativeTlsFtpStream::connect(&addr)
            .await
            .map_err(|e| DiskDeckError::Storage(format!("FTP connect: {e}")))?;

        if self.tls {
            let connector = suppaftp::AsyncNativeTlsConnector::from(
                suppaftp::async_native_tls::TlsConnector::new(),
            );
            ftp = ftp
                .into_secure(connector, &self.host)
                .await
                .map_err(|e| DiskDeckError::Storage(format!("FTP TLS upgrade: {e}")))?;
        }

        ftp.login(&self.username, &self.password)
            .await
            .map_err(|e| DiskDeckError::Storage(format!("FTP login: {e}")))?;

        // Use binary transfer mode to prevent newline translation
        ftp.transfer_type(suppaftp::types::FileType::Binary)
            .await
            .map_err(|e| DiskDeckError::Storage(format!("FTP binary mode: {e}")))?;

        Ok(ftp)
    }

    /// Returns a mutex guard to the FTP connection, connecting if necessary.
    async fn get_conn(
        &self,
    ) -> StorageResult<tokio::sync::MutexGuard<'_, Option<AsyncNativeTlsFtpStream>>> {
        let mut guard = self.conn.lock().await;
        if guard.is_none() {
            *guard = Some(self.connect().await?);
        }
        Ok(guard)
    }

    /// Drops the current connection so the next operation triggers a reconnect.
    async fn reset_conn(&self) {
        let mut guard = self.conn.lock().await;
        *guard = None;
    }

    /// Normalizes a DiskDeck virtual path to an FTP absolute path.
    ///
    /// FTP paths are typically absolute (starting with `/`), unlike SFTP
    /// which uses relative paths from the user's home directory.
    ///
    /// - `""` -> `"/"`
    /// - `"foo"` -> `"/foo"`
    /// - `"/foo"` -> `"/foo"` (unchanged)
    fn normalize_path(path: &str) -> String {
        if path.is_empty() {
            "/".to_string()
        } else if !path.starts_with('/') {
            format!("/{}", path)
        } else {
            path.to_string()
        }
    }

    /// Builds a DiskDeck virtual entry path from an FTP directory path and filename.
    fn to_entry_path(dir: &str, name: &str) -> String {
        let base = dir.trim_end_matches('/');
        if base.is_empty() || base == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", base, name)
        }
    }
}

/// Parses a single line from a Unix-style FTP `LIST` response into an [`Entry`].
///
/// Expected format:
/// ```text
/// drwxr-xr-x  2 user group 4096 Jan  1 12:00 dirname
/// -rw-r--r--  1 user group 1234 Jan  1 12:00 filename.txt
/// ```
///
/// Returns `None` for lines that cannot be parsed, or for `.` and `..` entries.
///
/// # Arguments
///
/// * `line` — A single line from the FTP LIST response.
/// * `dir_path` — The directory that was listed (used to build the entry path).
fn parse_list_line(line: &str, dir_path: &str) -> Option<Entry> {
    // Format: drwxr-xr-x  2 user group 4096 Jan  1 12:00 name
    //    or:  -rw-r--r--  1 user group 1234 Jan  1 12:00 name
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 9 {
        return None;
    }

    let perms = parts[0];
    let is_dir = perms.starts_with('d');
    let size: u64 = parts[4].parse().unwrap_or(0);
    // Name is everything after the 8th space-separated field.
    // This handles filenames containing spaces (e.g., "my report.pdf").
    let name_start = line
        .split_whitespace()
        .take(8)
        .fold(0, |acc, part| {
            line[acc..]
                .find(part)
                .map(|i| acc + i + part.len())
                .unwrap_or(acc)
        });
    let name = line[name_start..].trim().to_string();

    if name.is_empty() || name == "." || name == ".." {
        return None;
    }

    let entry_path = FtpBackend::to_entry_path(dir_path, &name);
    let mime = if !is_dir {
        mime_guess::from_path(&name)
            .first()
            .map(|m| m.to_string())
    } else {
        None
    };

    Some(Entry {
        path: entry_path,
        name,
        size,
        modified: None,
                    created: None,
        is_dir,
        permissions: Some(perms.to_string()),
        mime_type: mime,
    })
}

#[async_trait]
impl StorageBackend for FtpBackend {
    /// Lists a directory by issuing the FTP `LIST` command and parsing each
    /// line with [`parse_list_line`].
    async fn list(&self, path: &str) -> StorageResult<Vec<Entry>> {
        let remote_path = Self::normalize_path(path);
        let mut guard = self.get_conn().await?;
        let ftp = guard.as_mut().unwrap();

        let lines = match ftp.list(Some(&remote_path)).await {
            Ok(l) => l,
            Err(e) => {
                drop(guard);
                self.reset_conn().await;
                return Err(DiskDeckError::Storage(format!("FTP list: {e}")));
            }
        };

        let mut entries: Vec<Entry> = lines
            .iter()
            .filter_map(|line| parse_list_line(line, &remote_path))
            .collect();

        // Sort: directories first, then alphabetical (case-insensitive)
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(entries)
    }

    /// Reads a file using the FTP `RETR` command.
    ///
    /// The `retr` API from `suppaftp` takes a callback that receives an async
    /// reader for the data connection. We read the entire stream into a `Vec<u8>`.
    async fn read(&self, path: &str) -> StorageResult<Vec<u8>> {
        let remote_path = Self::normalize_path(path);
        let mut guard = self.get_conn().await?;
        let ftp = guard.as_mut().unwrap();

        let result = ftp
            .retr(
                &remote_path,
                |mut stream| -> Pin<Box<dyn Future<Output = suppaftp::FtpResult<(Vec<u8>, _)>> + Send>> {
                    Box::pin(async move {
                        let mut buf = Vec::new();
                        stream
                            .read_to_end(&mut buf)
                            .await
                            .map_err(suppaftp::FtpError::ConnectionError)?;
                        Ok((buf, stream))
                    })
                },
            )
            .await;

        match result {
            Ok(data) => Ok(data),
            Err(e) => {
                drop(guard);
                self.reset_conn().await;
                Err(DiskDeckError::Storage(format!("FTP read: {e}")))
            }
        }
    }

    /// Writes a file using the FTP `STOR` command.
    async fn write(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        let remote_path = Self::normalize_path(path);
        let mut guard = self.get_conn().await?;
        let ftp = guard.as_mut().unwrap();

        let mut cursor = futures::io::Cursor::new(data.to_vec());
        match ftp.put_file(&remote_path, &mut cursor).await {
            Ok(_) => Ok(()),
            Err(e) => {
                drop(guard);
                self.reset_conn().await;
                Err(DiskDeckError::Storage(format!("FTP write: {e}")))
            }
        }
    }

    /// Deletes a file or directory. Tries file removal (`DELE`) first;
    /// if that fails, falls back to directory removal (`RMD`).
    async fn delete(&self, path: &str) -> StorageResult<()> {
        let remote_path = Self::normalize_path(path);
        let mut guard = self.get_conn().await?;
        let ftp = guard.as_mut().unwrap();

        match ftp.rm(&remote_path).await {
            Ok(()) => Ok(()),
            Err(_) => match ftp.rmdir(&remote_path).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    drop(guard);
                    self.reset_conn().await;
                    Err(DiskDeckError::Storage(format!("FTP delete: {e}")))
                }
            },
        }
    }

    /// Copies by downloading then re-uploading (FTP has no server-side copy).
    async fn copy(&self, src: &str, dst: &str) -> StorageResult<()> {
        let data = self.read(src).await?;
        self.write(dst, &data).await
    }

    /// Renames using the FTP `RNFR`/`RNTO` command sequence.
    async fn rename(&self, src: &str, dst: &str) -> StorageResult<()> {
        let src_path = Self::normalize_path(src);
        let dst_path = Self::normalize_path(dst);
        let mut guard = self.get_conn().await?;
        let ftp = guard.as_mut().unwrap();

        match ftp.rename(&src_path, &dst_path).await {
            Ok(()) => Ok(()),
            Err(e) => {
                drop(guard);
                self.reset_conn().await;
                Err(DiskDeckError::Storage(format!("FTP rename: {e}")))
            }
        }
    }

    /// Returns metadata for a single entry by listing the parent directory
    /// and finding the matching entry by name.
    ///
    /// This is a workaround because FTP's `STAT` command is not reliable
    /// across server implementations.
    async fn stat(&self, path: &str) -> StorageResult<Entry> {
        let remote_path = Self::normalize_path(path);
        let parent = std::path::Path::new(&remote_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());

        let name = std::path::Path::new(&remote_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let entries = self.list(&parent).await?;
        entries
            .into_iter()
            .find(|e| e.name == name)
            .ok_or_else(|| DiskDeckError::NotFound(format!("FTP: {} not found", remote_path)))
    }

    async fn exists(&self, path: &str) -> StorageResult<bool> {
        match self.stat(path).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Searches by walking the directory tree via repeated `LIST` calls.
    ///
    /// Uses iterative depth-first traversal. Errors reading a directory
    /// are silently skipped (returns an empty list for that directory).
    async fn search(&self, query: &SearchQuery) -> StorageResult<Vec<Entry>> {
        let pattern = query.pattern.to_lowercase();
        let recursive = query.recursive;

        let mut results = Vec::new();
        let mut dirs_to_visit = vec!["/".to_string()];

        while let Some(dir) = dirs_to_visit.pop() {
            let entries = self.list(&dir).await.unwrap_or_default();

            for entry in entries {
                if entry.name.to_lowercase().contains(&pattern) {
                    results.push(entry.clone());
                }
                if recursive && entry.is_dir {
                    dirs_to_visit.push(entry.path.clone());
                }
            }
        }

        Ok(results)
    }

    /// Creates a directory using the FTP `MKD` command.
    async fn create_dir(&self, path: &str) -> StorageResult<()> {
        let remote_path = Self::normalize_path(path);
        let mut guard = self.get_conn().await?;
        let ftp = guard.as_mut().unwrap();

        match ftp.mkdir(&remote_path).await {
            Ok(()) => Ok(()),
            Err(e) => {
                drop(guard);
                self.reset_conn().await;
                Err(DiskDeckError::Storage(format!("FTP mkdir: {e}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_list_line_file() {
        let line = "-rw-r--r--  1 user group 1234 Jan  1 12:00 hello.txt";
        let entry = parse_list_line(line, "/").unwrap();
        assert_eq!(entry.name, "hello.txt");
        assert_eq!(entry.path, "/hello.txt");
        assert_eq!(entry.size, 1234);
        assert!(!entry.is_dir);
        assert_eq!(entry.permissions.as_deref(), Some("-rw-r--r--"));
    }

    #[test]
    fn parse_list_line_dir() {
        let line = "drwxr-xr-x  2 user group 4096 Mar 15 09:30 documents";
        let entry = parse_list_line(line, "/home").unwrap();
        assert_eq!(entry.name, "documents");
        assert_eq!(entry.path, "/home/documents");
        assert!(entry.is_dir);
    }

    #[test]
    fn parse_list_line_skips_dot() {
        let line = "drwxr-xr-x  2 user group 4096 Jan  1 12:00 .";
        assert!(parse_list_line(line, "/").is_none());
    }

    #[test]
    fn parse_list_line_skips_dotdot() {
        let line = "drwxr-xr-x  2 user group 4096 Jan  1 12:00 ..";
        assert!(parse_list_line(line, "/").is_none());
    }

    #[test]
    fn parse_list_line_short_line() {
        assert!(parse_list_line("drwx  2 user", "/").is_none());
    }

    #[test]
    fn normalize_path_empty() {
        assert_eq!(FtpBackend::normalize_path(""), "/");
    }

    #[test]
    fn normalize_path_no_slash() {
        assert_eq!(FtpBackend::normalize_path("foo"), "/foo");
    }

    #[test]
    fn normalize_path_with_slash() {
        assert_eq!(FtpBackend::normalize_path("/foo"), "/foo");
    }

    #[test]
    fn to_entry_path_root() {
        assert_eq!(FtpBackend::to_entry_path("/", "file.txt"), "/file.txt");
    }

    #[test]
    fn to_entry_path_nested() {
        assert_eq!(
            FtpBackend::to_entry_path("/home/user", "docs"),
            "/home/user/docs"
        );
    }

    #[test]
    fn to_entry_path_trailing_slash() {
        assert_eq!(FtpBackend::to_entry_path("/home/", "file.txt"), "/home/file.txt");
    }

    #[test]
    fn to_entry_path_empty_base() {
        assert_eq!(FtpBackend::to_entry_path("", "file.txt"), "/file.txt");
    }

    #[test]
    fn parse_list_line_file_with_mime() {
        let line = "-rw-r--r--  1 user group 5000 Feb 10 14:30 report.pdf";
        let entry = parse_list_line(line, "/docs").unwrap();
        assert_eq!(entry.name, "report.pdf");
        assert_eq!(entry.path, "/docs/report.pdf");
        assert_eq!(entry.size, 5000);
        assert!(!entry.is_dir);
        assert!(entry.mime_type.is_some());
        assert!(entry.mime_type.unwrap().contains("pdf"));
    }

    #[test]
    fn parse_list_line_dir_no_mime() {
        let line = "drwxr-xr-x  5 user group 4096 Mar 15 09:30 my_folder";
        let entry = parse_list_line(line, "/").unwrap();
        assert!(entry.is_dir);
        assert!(entry.mime_type.is_none());
    }

    #[test]
    fn parse_list_line_filename_with_spaces() {
        let line = "-rw-r--r--  1 user group 1234 Jan  1 12:00 my report.pdf";
        let entry = parse_list_line(line, "/").unwrap();
        assert_eq!(entry.name, "my report.pdf");
    }

    #[test]
    fn new_creates_backend() {
        let backend = FtpBackend::new("example.com", 21, "user", "pass", false);
        assert_eq!(backend.host, "example.com");
        assert_eq!(backend.port, 21);
        assert!(!backend.tls);
    }

    #[test]
    fn new_creates_tls_backend() {
        let backend = FtpBackend::new("example.com", 990, "user", "pass", true);
        assert!(backend.tls);
    }
}
