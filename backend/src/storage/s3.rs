//! # AWS S3 storage backend
//!
//! Implements [`StorageBackend`] for Amazon S3 (and S3-compatible stores).
//!
//! ## S3 folder emulation
//!
//! S3 is a flat key-value store with no true directory hierarchy. DiskDeck
//! emulates folders using the standard S3 conventions:
//!
//! - **Listing**: Uses `delimiter="/"` with `ListObjectsV2` so S3 returns
//!   `CommonPrefixes` (virtual folders) and `Contents` (files) for a given prefix.
//! - **Creating folders**: Writes a zero-byte object with a trailing-slash key
//!   (e.g., `documents/`).
//! - **Deleting folders**: Lists all objects under the prefix and deletes each one
//!   individually (S3 has no recursive-delete API).
//! - **Renaming**: Performed as copy-then-delete because S3 has no native rename.
//!
//! ## Key normalization
//!
//! DiskDeck uses virtual paths with a leading `/` (e.g., `/photos/sunset.jpg`),
//! but S3 keys should not have a leading slash. The [`S3Backend::normalize_key`]
//! method strips the leading `/` before every API call, and
//! [`S3Backend::key_to_path`] adds it back when constructing [`Entry`] paths.
//!
//! ## Initialization
//!
//! Unlike local/SFTP/FTP backends, S3 requires an async call to build the AWS SDK
//! config (`aws_config::load()`). This is why S3 backends are restored in a
//! separate async task during app startup (see `lib.rs`).

use async_trait::async_trait;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;

use crate::error::DiskDeckError;
use crate::models::entry::Entry;
use crate::models::search::SearchQuery;

use super::{StorageBackend, StorageResult};

/// AWS S3 storage backend.
///
/// Holds a configured S3 client and the target bucket name.
/// All operations are scoped to a single bucket.
pub struct S3Backend {
    /// The pre-configured AWS S3 client (includes region and credentials).
    client: Client,
    /// The S3 bucket name this backend operates on.
    bucket: String,
}

impl S3Backend {
    /// Creates an S3Backend from a pre-built client and bucket name.
    #[allow(dead_code)]
    pub fn new(client: Client, bucket: String) -> Self {
        Self { client, bucket }
    }

    /// Builds an S3Backend from explicit AWS credentials.
    ///
    /// This is the primary constructor used during disk creation and restoration.
    /// It builds a full AWS SDK config with the given region and credential pair.
    ///
    /// # Arguments
    ///
    /// * `bucket` — S3 bucket name.
    /// * `region` — AWS region (e.g., `"us-east-1"`).
    /// * `access_key_id` — AWS access key ID.
    /// * `secret_access_key` — AWS secret access key.
    ///
    /// # Errors
    ///
    /// Returns `DiskDeckError::Storage` if the AWS config cannot be built
    /// (network issues, invalid region, etc.).
    pub async fn from_credentials(
        bucket: &str,
        region: &str,
        access_key_id: &str,
        secret_access_key: &str,
    ) -> Result<Self, DiskDeckError> {
        let creds = aws_credential_types::Credentials::new(
            access_key_id,
            secret_access_key,
            None,
            None,
            "diskdeck",
        );
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .credentials_provider(creds)
            .load()
            .await;
        let client = Client::new(&config);
        Ok(Self {
            client,
            bucket: bucket.to_string(),
        })
    }

    /// Strips the leading `/` from a virtual path to produce a valid S3 key.
    ///
    /// Example: `"/photos/sunset.jpg"` -> `"photos/sunset.jpg"`.
    fn normalize_key(path: &str) -> &str {
        path.strip_prefix('/').unwrap_or(path)
    }

    /// Converts an S3 key back to a virtual absolute-style path.
    ///
    /// Example: `"photos/sunset.jpg"` -> `"/photos/sunset.jpg"`.
    fn key_to_path(key: &str) -> String {
        format!("/{}", key)
    }

    /// Extracts the "filename" (last path component) from an S3 key.
    ///
    /// Handles trailing slashes for folder keys.
    /// Example: `"documents/reports/"` -> `"reports"`.
    fn key_name(key: &str) -> String {
        let trimmed = key.trim_end_matches('/');
        trimmed
            .rsplit('/')
            .next()
            .unwrap_or(trimmed)
            .to_string()
    }
}

#[async_trait]
impl StorageBackend for S3Backend {
    /// Lists entries at the given virtual path using `ListObjectsV2` with a
    /// delimiter. Common prefixes become directory entries; objects become file
    /// entries. The prefix itself is excluded from results.
    async fn list(&self, path: &str) -> StorageResult<Vec<Entry>> {
        let mut prefix = Self::normalize_key(path).to_string();
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }

        let resp = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(&prefix)
            .delimiter("/")
            .send()
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?;

        let mut entries = Vec::new();

        // Folders (common prefixes returned by S3 when using delimiter)
        for cp in resp.common_prefixes() {
            if let Some(p) = cp.prefix() {
                let name = Self::key_name(p);
                if !name.is_empty() {
                    entries.push(Entry {
                        path: Self::key_to_path(p.trim_end_matches('/')),
                        name,
                        size: 0,
                        modified: None,
                        created: None,
                        is_dir: true,
                        permissions: None,
                        mime_type: None,
                    });
                }
            }
        }

        // Files (actual S3 objects at this prefix level)
        for obj in resp.contents() {
            if let Some(key) = obj.key() {
                // Skip the prefix object itself (the "folder marker")
                if key == prefix {
                    continue;
                }
                let name = Self::key_name(key);
                if name.is_empty() {
                    continue;
                }
                let modified = obj
                    .last_modified()
                    .and_then(|dt: &aws_sdk_s3::primitives::DateTime| dt.to_millis().ok())
                    .map(|ms| ms / 1000);
                let mime = mime_guess::from_path(&name)
                    .first()
                    .map(|m| m.to_string());

                entries.push(Entry {
                    path: Self::key_to_path(key),
                    name,
                    size: obj.size().unwrap_or(0) as u64,
                    modified,
                    created: None,
                    is_dir: false,
                    permissions: None,
                    mime_type: mime,
                });
            }
        }

        Ok(entries)
    }

    async fn read(&self, path: &str) -> StorageResult<Vec<u8>> {
        let key = Self::normalize_key(path);
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?;

        let data = resp
            .body
            .collect()
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?
            .into_bytes()
            .to_vec();

        Ok(data)
    }

    async fn write(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        let key = Self::normalize_key(path);
        let body = ByteStream::from(data.to_vec());

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body)
            .send()
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?;

        Ok(())
    }

    /// Deletes a file or "folder" from S3.
    ///
    /// Because S3 has no recursive delete, folder deletion requires:
    /// 1. Checking if the key is a folder prefix (has child objects).
    /// 2. If yes, listing all objects under that prefix and deleting each one.
    /// 3. If no, deleting the single object.
    async fn delete(&self, path: &str) -> StorageResult<()> {
        let key = Self::normalize_key(path);

        // Check if it's a "folder" by listing with prefix
        let folder_prefix = if key.ends_with('/') {
            key.to_string()
        } else {
            format!("{}/", key)
        };

        let resp = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(&folder_prefix)
            .max_keys(1)
            .send()
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?;

        if resp.key_count().unwrap_or(0) > 0 {
            // It's a folder — delete all objects under this prefix
            let list = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&folder_prefix)
                .send()
                .await
                .map_err(|e| DiskDeckError::Storage(e.to_string()))?;

            for obj in list.contents() {
                if let Some(k) = obj.key() {
                    self.client
                        .delete_object()
                        .bucket(&self.bucket)
                        .key(k)
                        .send()
                        .await
                        .map_err(|e| DiskDeckError::Storage(e.to_string()))?;
                }
            }
        } else {
            // Single object deletion
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
                .map_err(|e| DiskDeckError::Storage(e.to_string()))?;
        }

        Ok(())
    }

    /// Copies a single object within the same bucket using S3's server-side copy.
    ///
    /// The `CopySource` format is `"bucket/key"` as required by the S3 API.
    /// Note: this does not handle folder copies (recursive prefix copies).
    async fn copy(&self, src: &str, dst: &str) -> StorageResult<()> {
        let src_key = Self::normalize_key(src);
        let dst_key = Self::normalize_key(dst);
        let copy_source = format!("{}/{}", self.bucket, src_key);

        self.client
            .copy_object()
            .bucket(&self.bucket)
            .copy_source(&copy_source)
            .key(dst_key)
            .send()
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?;

        Ok(())
    }

    /// Renames by copying then deleting, since S3 has no native rename operation.
    async fn rename(&self, src: &str, dst: &str) -> StorageResult<()> {
        self.copy(src, dst).await?;
        self.delete(src).await?;
        Ok(())
    }

    /// Retrieves metadata for a single object using `HeadObject`.
    ///
    /// MIME type is taken from the S3 `Content-Type` header if present,
    /// falling back to extension-based guessing.
    async fn stat(&self, path: &str) -> StorageResult<Entry> {
        let key = Self::normalize_key(path);

        let resp = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?;

        let name = Self::key_name(key);
        let modified = resp
            .last_modified()
            .and_then(|dt: &aws_sdk_s3::primitives::DateTime| dt.to_millis().ok())
            .map(|ms| ms / 1000);
        let mime = resp
            .content_type()
            .map(|s| s.to_string())
            .or_else(|| mime_guess::from_path(&name).first().map(|m| m.to_string()));

        Ok(Entry {
            path: Self::key_to_path(key),
            name,
            size: resp.content_length().unwrap_or(0) as u64,
            modified,
            created: None,
            is_dir: false,
            permissions: None,
            mime_type: mime,
        })
    }

    /// Checks existence by issuing a `HeadObject` request.
    /// Any error (including 404) is treated as "does not exist".
    async fn exists(&self, path: &str) -> StorageResult<bool> {
        let key = Self::normalize_key(path);
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Brute-force search: lists all objects in the bucket and filters by name.
    ///
    /// This is the live-search fallback for unindexed S3 disks. For large buckets
    /// this can be slow; the FTS5 index should be used when available.
    ///
    /// Note: only matches against the filename portion of the key, not the full
    /// path. Pagination is not implemented (only the first page of results is
    /// returned by `ListObjectsV2`).
    async fn search(&self, query: &SearchQuery) -> StorageResult<Vec<Entry>> {
        let mut entries = Vec::new();
        let pattern = query.pattern.to_lowercase();

        let resp = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .send()
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?;

        for obj in resp.contents() {
            if let Some(key) = obj.key() {
                let name = Self::key_name(key);
                if name.to_lowercase().contains(&pattern) {
                    let modified = obj
                        .last_modified()
                        .and_then(|dt: &aws_sdk_s3::primitives::DateTime| dt.to_millis().ok())
                        .map(|ms| ms / 1000);
                    let mime = mime_guess::from_path(&name)
                        .first()
                        .map(|m| m.to_string());

                    entries.push(Entry {
                        path: Self::key_to_path(key),
                        name,
                        size: obj.size().unwrap_or(0) as u64,
                        modified,
                        created: None,
                        is_dir: false,
                        permissions: None,
                        mime_type: mime,
                    });
                }
            }
        }

        Ok(entries)
    }

    /// Creates a "folder" by writing a zero-byte object with a trailing-slash key.
    ///
    /// This is the standard S3 convention for representing empty directories.
    /// The object appears as a `CommonPrefix` in subsequent `list()` calls.
    async fn create_dir(&self, path: &str) -> StorageResult<()> {
        let mut key = Self::normalize_key(path).to_string();
        if !key.ends_with('/') {
            key.push('/');
        }

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(ByteStream::from(Vec::<u8>::new()))
            .send()
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_key_strips_leading_slash() {
        assert_eq!(S3Backend::normalize_key("/photos/sunset.jpg"), "photos/sunset.jpg");
    }

    #[test]
    fn normalize_key_no_slash() {
        assert_eq!(S3Backend::normalize_key("photos/sunset.jpg"), "photos/sunset.jpg");
    }

    #[test]
    fn normalize_key_just_slash() {
        assert_eq!(S3Backend::normalize_key("/"), "");
    }

    #[test]
    fn normalize_key_empty() {
        assert_eq!(S3Backend::normalize_key(""), "");
    }

    #[test]
    fn key_to_path_adds_slash() {
        assert_eq!(S3Backend::key_to_path("photos/sunset.jpg"), "/photos/sunset.jpg");
    }

    #[test]
    fn key_to_path_empty() {
        assert_eq!(S3Backend::key_to_path(""), "/");
    }

    #[test]
    fn key_name_simple() {
        assert_eq!(S3Backend::key_name("photos/sunset.jpg"), "sunset.jpg");
    }

    #[test]
    fn key_name_trailing_slash() {
        assert_eq!(S3Backend::key_name("documents/reports/"), "reports");
    }

    #[test]
    fn key_name_no_slash() {
        assert_eq!(S3Backend::key_name("file.txt"), "file.txt");
    }

    #[test]
    fn key_name_empty() {
        assert_eq!(S3Backend::key_name(""), "");
    }

    #[tokio::test]
    async fn from_credentials_creates_backend() {
        let result = S3Backend::from_credentials("mybucket", "us-east-1", "AKIA", "secret").await;
        assert!(result.is_ok());
        let backend = result.unwrap();
        assert_eq!(backend.bucket, "mybucket");
    }
}
